//! The integrated terminal: a shell in a pseudo-terminal, its output parsed
//! by `vt100` into a grid of cells, drawn on a canvas.
//!
//! A [`Session`] owns the shell. A reader thread feeds what it prints to the
//! parser; the canvas ([`view`]) paints the parser's screen and, while it
//! has the focus, turns key presses into the bytes a terminal sends and
//! writes them to the shell.

use std::io::{Read, Write};
use std::num::NonZeroU16;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Weak};

use iced::keyboard::{self, key::Named, Key};
use iced::mouse;
use iced::widget::canvas::{self, Canvas, Frame, Geometry, Text};
use iced::{Color, Element, Length, Pixels, Point, Rectangle, Size, Theme};
use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};

use crate::app::Message;
use crate::theme;

pub const FONT_SIZE: f32 = 13.0;
/// JetBrains Mono's advance is 0.6 em.
const CELL_W: f32 = FONT_SIZE * 0.6;
const CELL_H: f32 = 18.0;
const PAD_X: f32 = 10.0;
const PAD_Y: f32 = 6.0;
const SCROLLBACK: usize = 5_000;

/// Counts what the shells print (and their ends): the repaint waits for it.
static OUTPUT: AtomicU64 = AtomicU64::new(0);

/// The replies to the queries found in what a program wrote: the position of
/// the cursor (`CSI 6 n`), the status (`CSI 5 n`) and the kind of terminal
/// (`CSI c`, `CSI > c`).
fn queries_answered(chunk: &[u8], screen: &vt100::Screen) -> Vec<u8> {
    let mut reply = Vec::new();
    let has = |needle: &[u8]| chunk.windows(needle.len()).any(|w| w == needle);
    if has(b"\x1b[6n") {
        let (row, col) = screen.cursor_position();
        reply.extend(format!("\x1b[{};{}R", row + 1, col + 1).bytes());
    }
    if has(b"\x1b[5n") {
        reply.extend(b"\x1b[0n");
    }
    if has(b"\x1b[c") || has(b"\x1b[0c") {
        reply.extend(b"\x1b[?1;2c");
    }
    if has(b"\x1b[>c") || has(b"\x1b[>0c") {
        reply.extend(b"\x1b[>0;0;0c");
    }
    reply
}

/// The bytes that tell a program the mouse did something: `button` is the
/// code of the xterm protocol (0 left, 64 and 65 the wheel, +32 for motion),
/// `release` ends a press; the cell is counted from 0.
fn mouse_report(
    encoding: vt100::MouseProtocolEncoding,
    button: u8,
    release: bool,
    (row, col): (u16, u16),
) -> Vec<u8> {
    let (x, y) = (col + 1, row + 1);
    match encoding {
        vt100::MouseProtocolEncoding::Sgr => {
            format!("\x1b[<{button};{x};{y}{}", if release { 'm' } else { 'M' }).into_bytes()
        }
        // The older ways: a release has no button, and the places stop
        // where a byte does.
        _ => {
            let code = if release { 3 } else { button };
            let byte = |n: u16| (n.min(223) + 32) as u8;
            vec![0x1b, b'[', b'M', code + 32, byte(x), byte(y)]
        }
    }
}

/// The word of a row of the screen at a column, without the punctuation
/// that ends a sentence.
fn word_at(row: &str, col: usize) -> Option<String> {
    let chars: Vec<char> = row.chars().collect();
    let at = col.min(chars.len().checked_sub(1)?);
    if chars[at].is_whitespace() {
        return None;
    }
    let mut start = at;
    while start > 0 && !chars[start - 1].is_whitespace() {
        start -= 1;
    }
    let mut end = at;
    while end + 1 < chars.len() && !chars[end + 1].is_whitespace() {
        end += 1;
    }
    let word: String = chars[start..=end].iter().collect();
    Some(
        word.trim_matches(|c| matches!(c, ',' | ';' | '\'' | '"' | '`' | '(' | ')' | '[' | ']'))
            .to_string(),
    )
    .filter(|w| !w.is_empty())
}

/// A message, at most every 33 ms, while some shell printed or ended: an idle
/// terminal does not repaint the window.
pub fn changes() -> iced::Subscription<Message> {
    iced::Subscription::run(|| {
        iced::stream::channel(1, async |mut output| {
            use iced::futures::SinkExt;
            let mut seen = OUTPUT.load(Ordering::Relaxed);
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(33)).await;
                let now = OUTPUT.load(Ordering::Relaxed);
                if now != seen {
                    seen = now;
                    let _ = output.send(Message::TerminalTick).await;
                }
            }
        })
    })
}

fn nonzero(n: u16) -> NonZeroU16 {
    NonZeroU16::new(n.max(1)).unwrap_or(NonZeroU16::MIN)
}

// ─── The shell ──────────────────────────────────────────────────────────────

/// A shell running in a pseudo-terminal.
pub struct Session {
    parser: Mutex<vt100::Parser>,
    writer: Mutex<Box<dyn Write + Send>>,
    master: Mutex<Box<dyn MasterPty + Send>>,
    child: Mutex<Box<dyn Child + Send + Sync>>,
    alive: AtomicBool,
    size: Mutex<(u16, u16)>,
    /// Where the shell was started: what a relative path of its output is
    /// from.
    cwd: Mutex<PathBuf>,
    /// The line the last search found (counted from the bottom).
    found: Mutex<Option<usize>>,
}

impl Session {
    /// The user's shell, started in `cwd`.
    pub fn spawn_shell(cwd: &Path, rows: u16, cols: u16) -> Result<Arc<Session>, String> {
        let shell = std::env::var("SHELL")
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "/bin/sh".to_string());
        let mut command = CommandBuilder::new(shell);
        // macOS terminals start login shells, which is what sets the PATH.
        if cfg!(target_os = "macos") {
            command.arg("-l");
        }
        command.cwd(cwd);
        let session = Self::spawn_command(command, rows, cols)?;
        if let Ok(mut dir) = session.cwd.lock() {
            *dir = cwd.to_path_buf();
        }
        Ok(session)
    }

    /// The folder the shell was started in.
    pub fn start_dir(&self) -> PathBuf {
        self.cwd.lock().map(|d| d.clone()).unwrap_or_default()
    }

    pub fn spawn_command(
        mut command: CommandBuilder,
        rows: u16,
        cols: u16,
    ) -> Result<Arc<Session>, String> {
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        let pair = native_pty_system()
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| e.to_string())?;
        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|e| e.to_string())?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;

        let session = Arc::new(Session {
            parser: Mutex::new(vt100::Parser::new(nonzero(rows), nonzero(cols), SCROLLBACK)),
            writer: Mutex::new(writer),
            master: Mutex::new(pair.master),
            child: Mutex::new(child),
            alive: AtomicBool::new(true),
            size: Mutex::new((rows, cols)),
            cwd: Mutex::new(PathBuf::new()),
            found: Mutex::new(None),
        });

        // The thread holds only a weak reference: dropping the session kills
        // the shell, which ends the read.
        let weak: Weak<Session> = Arc::downgrade(&session);
        std::thread::spawn(move || {
            let mut buffer = [0u8; 8192];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let Some(session) = weak.upgrade() else { break };
                        let reply = match session.parser.lock() {
                            Ok(mut parser) => {
                                parser.process(&buffer[..n]);
                                queries_answered(&buffer[..n], parser.screen())
                            }
                            Err(_) => Vec::new(),
                        };
                        // What a program asks of the terminal (where the
                        // cursor is, what the terminal is) is answered, or
                        // it waits for a reply that never comes.
                        if !reply.is_empty() {
                            session.write(&reply);
                        }
                        OUTPUT.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }
            if let Some(session) = weak.upgrade() {
                session.alive.store(false, Ordering::Relaxed);
            }
            OUTPUT.fetch_add(1, Ordering::Relaxed);
        });
        Ok(session)
    }

    /// Sends bytes to the shell, as typed.
    pub fn write(&self, bytes: &[u8]) {
        if let Ok(mut writer) = self.writer.lock() {
            let _ = writer.write_all(bytes);
            let _ = writer.flush();
        }
        // Typing brings the view back to the live screen.
        self.scroll_to_bottom();
    }

    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::Relaxed)
    }

    /// Resizes the pseudo-terminal and the screen, if the size changed.
    pub fn resize(&self, rows: u16, cols: u16) {
        let (rows, cols) = (rows.max(1), cols.max(2));
        let Ok(mut size) = self.size.lock() else {
            return;
        };
        if *size == (rows, cols) {
            return;
        }
        *size = (rows, cols);
        if let Ok(master) = self.master.lock() {
            let _ = master.resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            });
        }
        if let Ok(mut parser) = self.parser.lock() {
            parser.set_size(nonzero(rows), nonzero(cols));
        }
    }

    /// Reads the screen.
    pub fn with_screen<R>(&self, f: impl FnOnce(&vt100::Screen) -> R) -> Option<R> {
        self.parser.lock().ok().map(|parser| f(parser.screen()))
    }

    /// Scrolls the history: positive goes up, towards older lines.
    /// Looks for `query` (whatever the case) in the screen and what has gone
    /// above it, before the last place found (`older`) or after it, and
    /// scrolls to it. Whether there was one.
    pub fn find(&self, query: &str, older: bool) -> bool {
        let query = query.to_lowercase();
        if query.is_empty() {
            return false;
        }
        let Ok(mut parser) = self.parser.lock() else {
            return false;
        };
        let screen = parser.screen_mut();
        let (rows, cols) = (screen.size().0.get() as usize, screen.size().1.get());
        let current = screen.scrollback();
        // Every line that holds it, as its place counted from the bottom.
        let mut lines: Vec<usize> = Vec::new();
        let mut offset = 0;
        loop {
            screen.set_scrollback(offset);
            let at = screen.scrollback();
            for (r, line) in screen.rows(0, cols).enumerate() {
                if line.to_lowercase().contains(&query) {
                    lines.push(at + rows - 1 - r);
                }
            }
            if at < offset || offset >= SCROLLBACK + rows {
                break;
            }
            offset += rows;
        }
        lines.sort_unstable();
        lines.dedup();
        let last = self.found.lock().ok().and_then(|f| *f);
        let from = last.map_or(current as isize - 1, |l| l as isize);
        let target = if older {
            lines.iter().copied().find(|l| *l as isize > from)
        } else {
            lines.iter().rev().copied().find(|l| (*l as isize) < from)
        };
        match target {
            Some(line) => {
                if let Ok(mut f) = self.found.lock() {
                    *f = Some(line);
                }
                screen.set_scrollback(line.saturating_sub(rows / 2));
                true
            }
            None => {
                screen.set_scrollback(current);
                false
            }
        }
    }

    /// The next search starts from the screen again.
    pub fn forget_found(&self) {
        if let Ok(mut f) = self.found.lock() {
            *f = None;
        }
    }

    pub fn scroll(&self, lines: i32) {
        if let Ok(mut parser) = self.parser.lock() {
            let screen = parser.screen_mut();
            let at = screen.scrollback() as i64 + lines as i64;
            screen.set_scrollback(at.max(0) as usize);
        }
    }

    fn scroll_to_bottom(&self) {
        if let Ok(mut parser) = self.parser.lock() {
            if parser.screen().scrollback() > 0 {
                parser.screen_mut().set_scrollback(0);
            }
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
        }
    }
}

// ─── The panel ──────────────────────────────────────────────────────────────

/// One terminal of the panel.
pub struct Tab {
    pub title: String,
    pub session: Arc<Session>,
}

/// The terminals at the bottom of the window.
#[derive(Default)]
pub struct Panel {
    pub open: bool,
    pub tabs: Vec<Tab>,
    pub active: usize,
    /// Whether the keyboard goes to the terminal.
    pub focused: bool,
    /// What is typed in the search bar (none: no bar).
    pub search: Option<String>,
    started: usize,
}

impl Panel {
    /// A new terminal in `cwd`, in front.
    pub fn add(&mut self, cwd: &Path, local: &str) -> Result<(), String> {
        let session = Session::spawn_shell(cwd, 24, 80)?;
        self.started += 1;
        let title = if self.started == 1 {
            local.to_string()
        } else {
            format!("{local} ({})", self.started)
        };
        self.tabs.push(Tab { title, session });
        self.active = self.tabs.len() - 1;
        Ok(())
    }

    pub fn close(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.tabs.remove(index);
        }
        self.fit();
    }

    /// Drops the terminals whose shell ended; true if there were any.
    pub fn prune_ended(&mut self) -> bool {
        let before = self.tabs.len();
        self.tabs.retain(|tab| tab.session.is_alive());
        self.fit();
        self.tabs.len() != before
    }

    pub fn active_session(&self) -> Option<&Arc<Session>> {
        self.tabs.get(self.active).map(|tab| &tab.session)
    }

    /// Keeps `active` valid; with no terminal left the panel closes.
    fn fit(&mut self) {
        if self.active >= self.tabs.len() {
            self.active = self.tabs.len().saturating_sub(1);
        }
        if self.tabs.is_empty() {
            self.open = false;
            self.focused = false;
        }
    }
}

// ─── Keys ───────────────────────────────────────────────────────────────────

/// The bytes a terminal sends for a key press, if it sends any.
pub fn encode_key(
    key: &Key,
    modifiers: keyboard::Modifiers,
    text: Option<&str>,
    application_cursor: bool,
) -> Option<Vec<u8>> {
    let (shift, alt, ctrl) = (modifiers.shift(), modifiers.alt(), modifiers.control());
    // xterm's modifier parameter: 1 + shift + 2 alt + 4 ctrl.
    let param = 1 + u8::from(shift) + 2 * u8::from(alt) + 4 * u8::from(ctrl);
    let csi = |final_byte: char| -> Vec<u8> {
        if param > 1 {
            format!("\x1b[1;{param}{final_byte}").into_bytes()
        } else if application_cursor {
            format!("\x1bO{final_byte}").into_bytes()
        } else {
            format!("\x1b[{final_byte}").into_bytes()
        }
    };
    let tilde = |n: u8| -> Vec<u8> {
        if param > 1 {
            format!("\x1b[{n};{param}~").into_bytes()
        } else {
            format!("\x1b[{n}~").into_bytes()
        }
    };
    let function = |n: u8| -> Vec<u8> {
        match n {
            1..=4 => {
                let letter = (b'P' + n - 1) as char;
                if param > 1 {
                    format!("\x1b[1;{param}{letter}").into_bytes()
                } else {
                    format!("\x1bO{letter}").into_bytes()
                }
            }
            _ => tilde(match n {
                5 => 15,
                6 => 17,
                7 => 18,
                8 => 19,
                9 => 20,
                10 => 21,
                11 => 23,
                _ => 24,
            }),
        }
    };

    if let Key::Named(named) = key {
        let bytes = match named {
            Named::Enter => vec![b'\r'],
            Named::Tab if shift => b"\x1b[Z".to_vec(),
            Named::Tab => vec![b'\t'],
            Named::Backspace if alt => vec![0x1b, 0x7f],
            Named::Backspace => vec![0x7f],
            Named::Escape => vec![0x1b],
            Named::Space if ctrl => vec![0],
            Named::Space => b" ".to_vec(),
            Named::ArrowUp => csi('A'),
            Named::ArrowDown => csi('B'),
            Named::ArrowRight => csi('C'),
            Named::ArrowLeft => csi('D'),
            Named::Home => csi('H'),
            Named::End => csi('F'),
            Named::Insert => tilde(2),
            Named::Delete => tilde(3),
            Named::PageUp => tilde(5),
            Named::PageDown => tilde(6),
            Named::F1 => function(1),
            Named::F2 => function(2),
            Named::F3 => function(3),
            Named::F4 => function(4),
            Named::F5 => function(5),
            Named::F6 => function(6),
            Named::F7 => function(7),
            Named::F8 => function(8),
            Named::F9 => function(9),
            Named::F10 => function(10),
            Named::F11 => function(11),
            Named::F12 => function(12),
            _ => return None,
        };
        return Some(bytes);
    }

    let Key::Character(typed) = key else {
        return None;
    };
    let mut chars = typed.chars();
    let first = chars.next()?;
    // Control + a letter (or one of a few symbols) is a control character.
    if ctrl && chars.next().is_none() {
        let control = match first.to_ascii_lowercase() {
            c @ 'a'..='z' => Some(c as u8 - b'a' + 1),
            '@' | ' ' | '2' => Some(0),
            '[' | '3' => Some(0x1b),
            '\\' | '4' => Some(0x1c),
            ']' | '5' => Some(0x1d),
            '^' | '6' => Some(0x1e),
            '_' | '-' | '7' => Some(0x1f),
            _ => None,
        };
        if let Some(byte) = control {
            let mut out = Vec::with_capacity(2);
            if alt {
                out.push(0x1b);
            }
            out.push(byte);
            return Some(out);
        }
    }
    // What the layout produced (dead keys, AltGr…), else the key itself.
    let produced = text
        .filter(|t| !t.is_empty() && !t.chars().any(char::is_control))
        .unwrap_or(typed.as_str());
    let mut out = Vec::new();
    if alt && !ctrl {
        out.push(0x1b);
    }
    out.extend_from_slice(produced.as_bytes());
    Some(out)
}

// ─── Colors ─────────────────────────────────────────────────────────────────

/// The 16 ANSI colors.
fn ansi(index: u8, dark: bool) -> Color {
    const DARK: [(u8, u8, u8); 16] = [
        (0x1b, 0x1d, 0x23),
        (0xf0, 0x6b, 0x6b),
        (0x8f, 0xd1, 0x8f),
        (0xe6, 0xc0, 0x6a),
        (0x6e, 0xa8, 0xff),
        (0xc6, 0x8a, 0xe8),
        (0x5e, 0xcf, 0xd1),
        (0xd0, 0xd3, 0xda),
        (0x5c, 0x61, 0x70),
        (0xff, 0x8a, 0x8a),
        (0xa8, 0xe8, 0xa8),
        (0xf5, 0xd5, 0x8a),
        (0x8f, 0xc0, 0xff),
        (0xdb, 0xa6, 0xf5),
        (0x84, 0xe3, 0xe5),
        (0xf2, 0xf3, 0xf6),
    ];
    const LIGHT: [(u8, u8, u8); 16] = [
        (0x1b, 0x1d, 0x23),
        (0xc6, 0x28, 0x28),
        (0x2e, 0x7d, 0x32),
        (0x9a, 0x6a, 0x00),
        (0x1b, 0x5f, 0xc6),
        (0x8e, 0x3a, 0xb8),
        (0x00, 0x7c, 0x85),
        (0x5c, 0x61, 0x70),
        (0x80, 0x85, 0x92),
        (0xd9, 0x3a, 0x3a),
        (0x3d, 0x9a, 0x42),
        (0xb0, 0x7d, 0x00),
        (0x2e, 0x74, 0xe0),
        (0xa4, 0x4f, 0xd0),
        (0x00, 0x96, 0xa0),
        (0x1b, 0x1d, 0x23),
    ];
    let (r, g, b) = if dark { DARK } else { LIGHT }[usize::from(index & 15)];
    Color::from_rgb8(r, g, b)
}

/// One of the 256 indexed colors: the 16 ANSI ones, a 6×6×6 cube, a gray ramp.
fn indexed(index: u8, dark: bool) -> Color {
    match index {
        0..=15 => ansi(index, dark),
        16..=231 => {
            let n = index - 16;
            let level = |v: u8| if v == 0 { 0 } else { 55 + 40 * v };
            Color::from_rgb8(level(n / 36), level(n / 6 % 6), level(n % 6))
        }
        _ => {
            let v = 8 + 10 * (index - 232);
            Color::from_rgb8(v, v, v)
        }
    }
}

fn resolve(color: vt100::Color, default: Color, dark: bool) -> Color {
    match color {
        vt100::Color::Default => default,
        vt100::Color::Idx(i) => indexed(i, dark),
        vt100::Color::Rgb(r, g, b) => Color::from_rgb8(r, g, b),
    }
}

// ─── The canvas ─────────────────────────────────────────────────────────────

/// The terminal on a canvas.
pub fn view<'a>(
    session: &'a Arc<Session>,
    dark: bool,
    focused: bool,
    search: &'a str,
) -> Element<'a, Message> {
    Canvas::new(Screen {
        session,
        dark,
        focused,
        search,
    })
    .width(Length::Fill)
    .height(Length::Fill)
    .into()
}

struct Screen<'a> {
    session: &'a Arc<Session>,
    dark: bool,
    focused: bool,
    /// What the search bar looks for (marked where it is on screen).
    search: &'a str,
}

/// A selection by dragging, in cells (row, column) of the visible screen.
#[derive(Default)]
struct Drag {
    anchor: Option<(u16, u16)>,
    head: Option<(u16, u16)>,
    dragging: bool,
    /// A program that asked for the mouse has the button down.
    reported: bool,
    modifiers: keyboard::Modifiers,
}

impl Drag {
    /// The selected cells in reading order, ends included.
    fn range(&self) -> Option<((u16, u16), (u16, u16))> {
        let (a, b) = (self.anchor?, self.head?);
        if a == b {
            return None;
        }
        Some(if a <= b { (a, b) } else { (b, a) })
    }
}

fn grid_size(bounds: Size) -> (u16, u16) {
    let cols = ((bounds.width - 2.0 * PAD_X) / CELL_W).floor().max(2.0) as u16;
    let rows = ((bounds.height - 2.0 * PAD_Y) / CELL_H).floor().max(1.0) as u16;
    (rows, cols)
}

fn cell_at(point: Point, rows: u16, cols: u16) -> (u16, u16) {
    let col = ((point.x - PAD_X) / CELL_W).floor().max(0.0) as u16;
    let row = ((point.y - PAD_Y) / CELL_H).floor().max(0.0) as u16;
    (row.min(rows - 1), col.min(cols - 1))
}

impl canvas::Program<Message> for Screen<'_> {
    type State = Drag;

    fn update(
        &self,
        drag: &mut Drag,
        event: &iced::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let (rows, cols) = grid_size(bounds.size());
        // A program that asked for the mouse (vim, htop, tmux) gets it; the
        // Shift key keeps the selection for the terminal.
        let (mode, encoding) = self
            .session
            .with_screen(|s| (s.mouse_protocol_mode(), s.mouse_protocol_encoding()))
            .unwrap_or((
                vt100::MouseProtocolMode::None,
                vt100::MouseProtocolEncoding::Default,
            ));
        let reporting = mode != vt100::MouseProtocolMode::None && !drag.modifiers.shift();
        match event {
            iced::Event::Keyboard(keyboard::Event::ModifiersChanged(m)) => {
                drag.modifiers = *m;
                None
            }
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                if reporting && self.focused && cursor.position_in(bounds).is_some() =>
            {
                let cell = cell_at(cursor.position_in(bounds)?, rows, cols);
                self.session.write(&mouse_report(encoding, 0, false, cell));
                drag.reported = true;
                Some(canvas::Action::capture())
            }
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                if drag.reported =>
            {
                drag.reported = false;
                if mode != vt100::MouseProtocolMode::Press {
                    if let Some(at) = cursor.position_in(bounds) {
                        let cell = cell_at(at, rows, cols);
                        self.session.write(&mouse_report(encoding, 0, true, cell));
                    }
                }
                Some(canvas::Action::capture())
            }
            iced::Event::Mouse(mouse::Event::CursorMoved { position })
                if drag.reported
                    && matches!(
                        mode,
                        vt100::MouseProtocolMode::ButtonMotion
                            | vt100::MouseProtocolMode::AnyMotion
                    ) =>
            {
                let at = Point::new(position.x - bounds.x, position.y - bounds.y);
                self.session
                    .write(&mouse_report(encoding, 32, false, cell_at(at, rows, cols)));
                Some(canvas::Action::capture())
            }
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
                if drag.modifiers.command() && cursor.position_in(bounds).is_some() =>
            {
                // Command (Ctrl elsewhere) and a click on `file:line`: opens it.
                let (row, col) = cell_at(cursor.position_in(bounds)?, rows, cols);
                let line = self
                    .session
                    .with_screen(|s| s.contents_between(row, 0, row, cols))
                    .unwrap_or_default();
                let place = word_at(&line, col as usize).unwrap_or(line);
                Some(
                    canvas::Action::publish(Message::TerminalOpen(place, self.session.start_dir()))
                        .and_capture(),
                )
            }
            iced::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                if let Some(at) = cursor.position_in(bounds) {
                    let cell = cell_at(at, rows, cols);
                    *drag = Drag {
                        anchor: Some(cell),
                        head: Some(cell),
                        dragging: true,
                        modifiers: drag.modifiers,
                        reported: false,
                    };
                    let action = if self.focused {
                        canvas::Action::request_redraw()
                    } else {
                        canvas::Action::publish(Message::TerminalFocus(true))
                    };
                    return Some(action.and_capture());
                }
                // A click elsewhere takes the keyboard back (the click goes on).
                *drag = Drag::default();
                self.focused
                    .then(|| canvas::Action::publish(Message::TerminalFocus(false)))
            }
            iced::Event::Mouse(mouse::Event::CursorMoved { position }) if drag.dragging => {
                let at = Point::new(position.x - bounds.x, position.y - bounds.y);
                drag.head = Some(cell_at(at, rows, cols));
                Some(canvas::Action::request_redraw())
            }
            iced::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
                if drag.dragging =>
            {
                drag.dragging = false;
                if drag.range().is_none() {
                    *drag = Drag::default();
                }
                Some(canvas::Action::request_redraw())
            }
            iced::Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                cursor.position_in(bounds)?;
                let lines = match delta {
                    mouse::ScrollDelta::Lines { y, .. } => y * 3.0,
                    mouse::ScrollDelta::Pixels { y, .. } => y / CELL_H,
                };
                let lines = lines.round() as i32;
                if lines == 0 {
                    return None;
                }
                if reporting {
                    let cell = cell_at(cursor.position_in(bounds)?, rows, cols);
                    for _ in 0..lines.unsigned_abs().min(10) {
                        let button = if lines > 0 { 64 } else { 65 };
                        self.session
                            .write(&mouse_report(encoding, button, false, cell));
                    }
                    return Some(canvas::Action::capture());
                }
                let alternate = self
                    .session
                    .with_screen(|s| s.alternate_screen())
                    .unwrap_or(false);
                if alternate {
                    // A full-screen program (less, vim) has no history: the
                    // wheel moves its cursor instead.
                    let key: &[u8] = if lines > 0 { b"\x1b[A" } else { b"\x1b[B" };
                    for _ in 0..lines.unsigned_abs().min(10) {
                        self.session.write(key);
                    }
                } else {
                    self.session.scroll(lines);
                }
                *drag = Drag::default();
                Some(canvas::Action::request_redraw().and_capture())
            }
            iced::Event::Keyboard(keyboard::Event::KeyPressed {
                key,
                modifiers,
                text,
                ..
            }) if self.focused => {
                // Copy and paste: Cmd+C / Cmd+V on macOS, Ctrl+Shift+C / V
                // elsewhere (plain Ctrl+C belongs to the shell).
                let chord = if cfg!(target_os = "macos") {
                    modifiers.command() && !modifiers.control()
                } else {
                    modifiers.control() && modifiers.shift()
                };
                if chord {
                    if let Key::Character(c) = key {
                        match c.to_lowercase().as_str() {
                            "c" => {
                                let selected = drag.range().and_then(|((r0, c0), (r1, c1))| {
                                    self.session.with_screen(|s| {
                                        s.contents_between(r0, c0, r1, c1.saturating_add(1))
                                    })
                                });
                                return Some(match selected {
                                    Some(text) if !text.is_empty() => {
                                        canvas::Action::publish(Message::TerminalCopy(text))
                                            .and_capture()
                                    }
                                    _ => canvas::Action::capture(),
                                });
                            }
                            "v" => {
                                return Some(
                                    canvas::Action::publish(Message::TerminalPaste).and_capture(),
                                );
                            }
                            "f" => {
                                return Some(
                                    canvas::Action::publish(Message::TerminalSearchToggle)
                                        .and_capture(),
                                );
                            }
                            _ => {}
                        }
                    }
                }
                if cfg!(target_os = "macos") && modifiers.command() {
                    // Other Cmd chords are the app's.
                    return None;
                }
                let application_cursor = self
                    .session
                    .with_screen(|s| s.application_cursor())
                    .unwrap_or(false);
                let bytes = encode_key(key, *modifiers, text.as_deref(), application_cursor)?;
                *drag = Drag::default();
                self.session.write(&bytes);
                Some(canvas::Action::request_redraw().and_capture())
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        drag: &Drag,
        renderer: &iced::Renderer,
        _theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let p = theme::palette(self.dark);
        let (rows, cols) = grid_size(bounds.size());
        self.session.resize(rows, cols);

        let mut frame = Frame::new(renderer, bounds.size());
        frame.fill_rectangle(Point::ORIGIN, bounds.size(), p.panel);

        let selection = drag.range();
        let selected = |row: u16, col: u16| {
            selection.is_some_and(|(start, end)| (row, col) >= start && (row, col) <= end)
        };
        let _ =
            self.session.with_screen(|screen| {
                let (cursor_row, cursor_col) = screen.cursor_position();
                let show_cursor = !screen.hide_cursor() && screen.scrollback() == 0;
                let shown_rows = rows.min(screen.size().0.get());
                let shown_cols = cols.min(screen.size().1.get());
                let needle = self.search.to_lowercase();
                for row in 0..shown_rows {
                    let y = PAD_Y + f32::from(row) * CELL_H;
                    // The columns of this row where the search text is.
                    let marks: Vec<(u16, u16)> = if needle.is_empty() {
                        Vec::new()
                    } else {
                        let line = screen
                            .contents_between(row, 0, row, shown_cols)
                            .to_lowercase();
                        line.match_indices(&needle)
                            .map(|(at, m)| {
                                let start = line[..at].chars().count() as u16;
                                (start, start + m.chars().count() as u16)
                            })
                            .collect()
                    };
                    // The text run being built: where it starts, and how it looks.
                    let mut run: Option<(u16, String, Color, bool, bool, bool)> = None;
                    let flush = |frame: &mut Frame,
                                 run: &mut Option<(u16, String, Color, bool, bool, bool)>| {
                    if let Some((start, text, color, bold, italic, underline)) = run.take() {
                        let x = PAD_X + f32::from(start) * CELL_W;
                        if underline {
                            let width = text.chars().count() as f32 * CELL_W;
                            frame.fill_rectangle(
                                Point::new(x, y + CELL_H - 2.0),
                                Size::new(width, 1.0),
                                color,
                            );
                        }
                        let mut font = theme::MONO;
                        // Only the regular and medium weights are bundled; a bold
                        // or italic face would fall back to a proportional font.
                        let _ = italic;
                        if bold {
                            font.weight = iced::font::Weight::Medium;
                        }
                        frame.fill_text(Text {
                            content: text,
                            position: Point::new(x, y),
                            color,
                            size: Pixels(FONT_SIZE),
                            line_height: iced::widget::text::LineHeight::Absolute(Pixels(CELL_H)),
                            font,
                            shaping: iced::widget::text::Shaping::Advanced,
                            ..Text::default()
                        });
                    }
                };
                    let mut col = 0;
                    while col < shown_cols {
                        let Some(cell) = screen.cell(row, col) else {
                            col += 1;
                            continue;
                        };
                        if cell.is_wide_continuation() {
                            col += 1;
                            continue;
                        }
                        let width = if cell.is_wide() { 2 } else { 1 };
                        let (mut fg, mut bg) = (
                            resolve(cell.fgcolor(), p.text, self.dark),
                            match cell.bgcolor() {
                                vt100::Color::Default => None,
                                other => Some(resolve(other, p.panel, self.dark)),
                            },
                        );
                        if cell.inverse() {
                            let shown_bg = bg.unwrap_or(p.panel);
                            bg = Some(fg);
                            fg = shown_bg;
                        }
                        if cell.dim() {
                            fg.a = 0.6;
                        }
                        if show_cursor && self.focused && (row, col) == (cursor_row, cursor_col) {
                            bg = Some(p.text);
                            fg = p.panel;
                        }
                        if marks.iter().any(|(a, b)| (*a..*b).contains(&col)) {
                            bg = Some(p.accent);
                            fg = p.panel;
                        }
                        if selected(row, col) {
                            bg = Some(p.accent_soft);
                            fg = p.text;
                        }
                        if let Some(color) = bg {
                            frame.fill_rectangle(
                                Point::new(PAD_X + f32::from(col) * CELL_W, y),
                                Size::new(f32::from(width) * CELL_W, CELL_H),
                                color,
                            );
                        }
                        if show_cursor && !self.focused && (row, col) == (cursor_row, cursor_col) {
                            frame.stroke_rectangle(
                                Point::new(PAD_X + f32::from(col) * CELL_W + 0.5, y + 0.5),
                                Size::new(CELL_W - 1.0, CELL_H - 1.0),
                                canvas::Stroke::default().with_color(p.muted),
                            );
                        }
                        let glyph = cell.contents();
                        let same_style = run.as_ref().is_some_and(|(_, _, c, b, i, u)| {
                            *c == fg
                                && *b == cell.bold()
                                && *i == cell.italic()
                                && *u == cell.underline()
                        });
                        if glyph.is_empty() || glyph == " " {
                            // A blank only matters inside a run (it keeps the columns).
                            if same_style || bg.is_none() {
                                if let Some((_, text, ..)) = run.as_mut() {
                                    text.push(' ');
                                }
                            } else {
                                // A styled blank (cursor, selection) ends the run: the
                                // next glyph starts at its own column.
                                flush(&mut frame, &mut run);
                            }
                            // Trailing blanks end the run lazily: trimmed on flush.
                        } else if width == 2 || !same_style {
                            flush(&mut frame, &mut run);
                            run = Some((
                                col,
                                glyph.to_string(),
                                fg,
                                cell.bold(),
                                cell.italic(),
                                cell.underline(),
                            ));
                            if width == 2 {
                                flush(&mut frame, &mut run);
                            }
                        } else if let Some((_, text, ..)) = run.as_mut() {
                            text.push_str(glyph);
                        }
                        col += width;
                    }
                    if let Some((_, text, ..)) = run.as_mut() {
                        let trimmed = text.trim_end().len();
                        text.truncate(trimmed);
                    }
                    flush(&mut frame, &mut run);
                }
            });
        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        _state: &Drag,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        if cursor.is_over(bounds) {
            mouse::Interaction::Text
        } else {
            mouse::Interaction::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::keyboard::Modifiers;

    fn key(c: &str) -> Key {
        Key::Character(c.into())
    }

    fn bytes(key: Key, modifiers: Modifiers, text: Option<&str>) -> Option<Vec<u8>> {
        encode_key(&key, modifiers, text, false)
    }

    #[test]
    fn typed_characters_go_through_as_utf8() {
        assert_eq!(
            bytes(key("a"), Modifiers::empty(), Some("a")),
            Some(b"a".to_vec())
        );
        assert_eq!(
            bytes(key("é"), Modifiers::empty(), Some("é")),
            Some("é".as_bytes().to_vec())
        );
        // The layout's own output wins (Shift, AltGr…).
        assert_eq!(
            bytes(key("a"), Modifiers::SHIFT, Some("A")),
            Some(b"A".to_vec())
        );
    }

    #[test]
    fn control_letters_are_control_characters() {
        assert_eq!(bytes(key("c"), Modifiers::CTRL, None), Some(vec![3]));
        assert_eq!(bytes(key("d"), Modifiers::CTRL, None), Some(vec![4]));
        assert_eq!(bytes(key("z"), Modifiers::CTRL, None), Some(vec![26]));
        assert_eq!(bytes(key("["), Modifiers::CTRL, None), Some(vec![0x1b]));
    }

    #[test]
    fn alt_prefixes_an_escape() {
        assert_eq!(
            bytes(key("b"), Modifiers::ALT, Some("b")),
            Some(vec![0x1b, b'b'])
        );
        assert_eq!(
            bytes(Key::Named(Named::Backspace), Modifiers::ALT, None),
            Some(vec![0x1b, 0x7f])
        );
    }

    #[test]
    fn named_keys_send_their_sequences() {
        let named = |n| bytes(Key::Named(n), Modifiers::empty(), None);
        assert_eq!(named(Named::Enter), Some(b"\r".to_vec()));
        assert_eq!(named(Named::Backspace), Some(vec![0x7f]));
        assert_eq!(named(Named::Tab), Some(b"\t".to_vec()));
        assert_eq!(named(Named::Escape), Some(vec![0x1b]));
        assert_eq!(named(Named::ArrowUp), Some(b"\x1b[A".to_vec()));
        assert_eq!(named(Named::Delete), Some(b"\x1b[3~".to_vec()));
        assert_eq!(named(Named::PageUp), Some(b"\x1b[5~".to_vec()));
        assert_eq!(named(Named::F1), Some(b"\x1bOP".to_vec()));
        assert_eq!(named(Named::F5), Some(b"\x1b[15~".to_vec()));
        assert_eq!(
            bytes(Key::Named(Named::Tab), Modifiers::SHIFT, None),
            Some(b"\x1b[Z".to_vec())
        );
        // Modifiers on an arrow: xterm's parameter.
        assert_eq!(
            bytes(Key::Named(Named::ArrowRight), Modifiers::CTRL, None),
            Some(b"\x1b[1;5C".to_vec())
        );
    }

    #[test]
    fn full_screen_programs_get_application_cursor_keys() {
        assert_eq!(
            encode_key(&Key::Named(Named::ArrowUp), Modifiers::empty(), None, true),
            Some(b"\x1bOA".to_vec())
        );
    }

    #[test]
    fn indexed_colors_cover_the_cube_and_the_gray_ramp() {
        assert_eq!(indexed(16, true), Color::from_rgb8(0, 0, 0));
        assert_eq!(indexed(231, true), Color::from_rgb8(255, 255, 255));
        assert_eq!(indexed(196, true), Color::from_rgb8(255, 0, 0));
        assert_eq!(indexed(232, true), Color::from_rgb8(8, 8, 8));
        assert_eq!(indexed(255, true), Color::from_rgb8(238, 238, 238));
    }

    #[test]
    fn the_grid_follows_the_size_of_the_canvas() {
        assert_eq!(grid_size(Size::new(800.0, 300.0)), (16, 100));
        // Never smaller than something that can show a prompt.
        assert_eq!(grid_size(Size::new(1.0, 1.0)), (1, 2));
    }

    #[test]
    fn a_selection_is_in_reading_order() {
        let drag = Drag {
            anchor: Some((3, 5)),
            head: Some((1, 2)),
            dragging: false,
            ..Drag::default()
        };
        assert_eq!(drag.range(), Some(((1, 2), (3, 5))));
        let click = Drag {
            anchor: Some((1, 1)),
            head: Some((1, 1)),
            dragging: false,
            ..Drag::default()
        };
        assert_eq!(click.range(), None);
    }

    fn wait_for(session: &Session, needle: &str) -> bool {
        for _ in 0..100 {
            let found = session
                .with_screen(|s| s.contents().contains(needle))
                .unwrap_or(false);
            if found {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        false
    }

    /// A real shell in a real pseudo-terminal: what is typed comes back
    /// printed, a resize reaches the shell, and the shell's end is noticed.
    #[cfg(unix)]
    #[test]
    fn a_shell_runs_in_the_session() {
        let mut command = CommandBuilder::new("sh");
        command.env("PS1", "$ ");
        let session = Session::spawn_command(command, 24, 80).expect("a shell");
        assert!(session.is_alive());
        session.write(b"echo hello-from-the-pty\n");
        assert!(wait_for(&session, "hello-from-the-pty"));

        session.resize(10, 40);
        session.write(b"stty size\n");
        assert!(wait_for(&session, "10 40"));

        session.write(b"exit\n");
        for _ in 0..100 {
            if !session.is_alive() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(!session.is_alive());
    }

    /// A program that asks where the cursor is gets the answer: the shell
    /// reads it from its input, and prints it.
    #[cfg(unix)]
    #[test]
    fn the_position_of_the_cursor_is_answered_to_a_program() {
        let mut command = CommandBuilder::new("bash");
        command.env("PS1", "$ ");
        let Ok(session) = Session::spawn_command(command, 24, 80) else {
            return;
        };
        session.write(b"printf '\\033[6n'; read -s -t 3 -d R reply; echo \"answer=${reply#*[}\"\n");
        assert!(wait_for(&session, "answer="));
        assert!(wait_for(&session, ";"), "a row and a column");
        session.write(b"exit\n");
    }

    #[test]
    fn the_mouse_is_reported_the_way_the_program_asked() {
        let sgr = mouse_report(vt100::MouseProtocolEncoding::Sgr, 0, false, (4, 9));
        assert_eq!(sgr, b"\x1b[<0;10;5M");
        let release = mouse_report(vt100::MouseProtocolEncoding::Sgr, 0, true, (4, 9));
        assert_eq!(release, b"\x1b[<0;10;5m");
        let old = mouse_report(vt100::MouseProtocolEncoding::Default, 64, false, (0, 0));
        assert_eq!(old, vec![0x1b, b'[', b'M', 96, 33, 33]);
    }

    #[test]
    fn the_word_under_a_click_loses_the_punctuation_around_it() {
        let row = "error at (src/app.rs:12:5), see";
        assert_eq!(word_at(row, 14).as_deref(), Some("src/app.rs:12:5"));
        assert_eq!(word_at(row, 5), None);
        assert_eq!(word_at("", 0), None);
    }

    #[test]
    fn the_queries_of_a_program_have_answers() {
        let mut parser = vt100::Parser::new(nonzero(24), nonzero(80), 0);
        parser.process(b"abc");
        let reply = queries_answered(b"\x1b[6n\x1b[c", parser.screen());
        assert_eq!(reply, b"\x1b[1;4R\x1b[?1;2c");
        assert!(queries_answered(b"plain", parser.screen()).is_empty());
    }

    #[test]
    fn a_search_goes_back_through_the_history_and_forth_again() {
        let mut command = CommandBuilder::new("sh");
        command.env("PS1", "$ ");
        let Ok(session) = Session::spawn_command(command, 5, 40) else {
            return;
        };
        session.write(
            b"echo needle-$((0+1)); seq 1 30; echo needle-$((1+1)); seq 31 60; echo end-$((5+5))\n",
        );
        assert!(wait_for(&session, "end-10"));
        // From the bottom: the last needle, then the one before, then none.
        assert!(session.find("NEEDLE-2", true));
        let first = session.with_screen(|s| s.contents()).unwrap();
        assert!(first.contains("needle-2"), "{first}");
        assert!(session.find("needle-1", true));
        let second = session.with_screen(|s| s.contents()).unwrap();
        assert!(second.contains("needle-1"), "{second}");
        assert!(!session.find("needle-1", true));
        // And forth again.
        assert!(session.find("needle-2", false));
        let back = session.with_screen(|s| s.contents()).unwrap();
        assert!(back.contains("needle-2"), "{back}");
        assert!(!session.find("absent", true));
    }
}
