//! Visual identity for SimpleEdit: "Nocturne".
//!
//! Deep night blue surfaces, floating rounded panels and one warm coral
//! accent (a darker coral in the light theme). Style helpers take
//! `dark: bool` directly instead of deriving colors from `iced::Theme`, since
//! the two custom themes only carry the base palette and we want exact
//! control over surfaces, borders and hairlines.

use iced::widget::{button, container, pick_list, scrollable, slider, text_input};
use iced::{font, Background, Border, Color, Font, Shadow, Theme, Vector};

pub const ACCENT_DARK: Color = rgb(0xFF, 0x9B, 0x6B);
pub const ACCENT_LIGHT: Color = rgb(0xD2, 0x56, 0x1F);

// Radii (px)
pub const R_PANEL: f32 = 14.0;
pub const R_PALETTE: f32 = 18.0;
pub const R_ROW: f32 = 10.0;
pub const R_SM: f32 = 8.0;
pub const R_MENU_TITLE: f32 = 6.0;
pub const R_PILL: f32 = 99.0;
/// Space between the floating panels, and the margin around them.
pub const PANEL_GAP: f32 = 10.0;

// Fonts. The files are embedded and registered at startup (see `main.rs`).
pub const UI: Font = Font::with_name("Instrument Sans");
pub const MONO: Font = Font::with_name("JetBrains Mono");

pub const UI_MEDIUM: Font = Font {
    weight: font::Weight::Medium,
    ..UI
};
pub const UI_SEMIBOLD: Font = Font {
    weight: font::Weight::Semibold,
    ..UI
};
pub const UI_BOLD: Font = Font {
    weight: font::Weight::Bold,
    ..UI
};
pub const UI_ITALIC: Font = Font {
    style: font::Style::Italic,
    ..UI
};
pub const UI_BOLD_ITALIC: Font = Font {
    weight: font::Weight::Bold,
    style: font::Style::Italic,
    ..UI
};

/// Every embedded font file, for `iced::Settings::fonts`.
pub fn font_files() -> Vec<std::borrow::Cow<'static, [u8]>> {
    [
        &include_bytes!("../assets/fonts/InstrumentSans-Regular.ttf")[..],
        &include_bytes!("../assets/fonts/InstrumentSans-Medium.ttf")[..],
        &include_bytes!("../assets/fonts/InstrumentSans-SemiBold.ttf")[..],
        &include_bytes!("../assets/fonts/InstrumentSans-Bold.ttf")[..],
        &include_bytes!("../assets/fonts/InstrumentSans-Italic.ttf")[..],
        &include_bytes!("../assets/fonts/InstrumentSans-BoldItalic.ttf")[..],
        &include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf")[..],
        &include_bytes!("../assets/fonts/JetBrainsMono-Medium.ttf")[..],
    ]
    .into_iter()
    .map(std::borrow::Cow::Borrowed)
    .collect()
}

pub fn ink_dark() -> Theme {
    Theme::custom(
        "SimpleEdit Nocturne".to_string(),
        iced::theme::Palette {
            background: rgb(0x07, 0x09, 0x0E),
            text: rgb(0xE9, 0xEC, 0xF4),
            primary: ACCENT_DARK,
            success: rgb(0x9B, 0xE3, 0xA4),
            danger: rgb(0xFF, 0x6B, 0x6B),
            warning: rgb(0xF5, 0xD0, 0x7A),
        },
    )
}

pub fn ink_light() -> Theme {
    Theme::custom(
        "SimpleEdit Nocturne Light".to_string(),
        iced::theme::Palette {
            background: rgb(0xE9, 0xEC, 0xF3),
            text: rgb(0x13, 0x17, 0x24),
            primary: ACCENT_LIGHT,
            success: rgb(0x1E, 0x8A, 0x4A),
            danger: rgb(0xC0, 0x42, 0x2D),
            warning: rgb(0x98, 0x6E, 0x0A),
        },
    )
}

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgb(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0)
}

const fn rgba(r: u8, g: u8, b: u8, a: f32) -> Color {
    Color {
        r: r as f32 / 255.0,
        g: g as f32 / 255.0,
        b: b as f32 / 255.0,
        a,
    }
}

/// Concrete color tokens for one mode (dark or light).
#[derive(Debug, Clone, Copy)]
pub struct Palette {
    /// Window background, input fields.
    pub bg: Color,
    /// Sidebar.
    pub surface: Color,
    /// Editor panel, cards.
    pub panel: Color,
    /// Menus, search card, palette.
    pub elevated: Color,
    pub hover: Color,
    /// Hairlines.
    pub border: Color,
    /// Borders of floating cards.
    pub border_strong: Color,
    pub text: Color,
    pub muted: Color,
    /// Line numbers.
    pub gutter: Color,
    pub accent: Color,
    /// Text on an accent background.
    pub accent_ink: Color,
    pub accent_soft: Color,
    pub current_line: Color,
    pub search_match: Color,
    pub scrim: Color,
}

pub fn palette(dark: bool) -> Palette {
    if dark {
        Palette {
            bg: rgb(0x07, 0x09, 0x0E),
            surface: rgb(0x0F, 0x12, 0x1B),
            panel: rgb(0x12, 0x16, 0x22),
            elevated: rgb(0x1A, 0x1F, 0x2E),
            hover: rgba(0xFF, 0xFF, 0xFF, 0.05),
            border: rgb(0x22, 0x28, 0x39),
            border_strong: rgb(0x2D, 0x35, 0x50),
            text: rgb(0xE9, 0xEC, 0xF4),
            muted: rgb(0x80, 0x89, 0xA0),
            gutter: rgb(0x5C, 0x63, 0x78),
            accent: ACCENT_DARK,
            accent_ink: rgb(0x1A, 0x0C, 0x05),
            accent_soft: rgba(0xFF, 0x9B, 0x6B, 0.14),
            current_line: rgba(0xFF, 0xFF, 0xFF, 0.035),
            search_match: rgba(0xFF, 0xC8, 0x50, 0.28),
            scrim: rgba(0x00, 0x00, 0x00, 0.5),
        }
    } else {
        Palette {
            bg: rgb(0xE9, 0xEC, 0xF3),
            surface: rgb(0xF7, 0xF8, 0xFB),
            panel: rgb(0xFF, 0xFF, 0xFF),
            elevated: rgb(0xFF, 0xFF, 0xFF),
            hover: rgba(0x13, 0x17, 0x24, 0.05),
            border: rgb(0xDD, 0xE1, 0xEA),
            border_strong: rgb(0xC9, 0xCF, 0xDC),
            text: rgb(0x13, 0x17, 0x24),
            muted: rgb(0x6B, 0x73, 0x88),
            gutter: rgb(0x8F, 0x94, 0xA0),
            accent: ACCENT_LIGHT,
            accent_ink: rgb(0xFF, 0xFF, 0xFF),
            accent_soft: rgba(0xD2, 0x56, 0x1F, 0.10),
            current_line: rgba(0x00, 0x00, 0x00, 0.035),
            search_match: rgba(0xFF, 0xBE, 0x28, 0.45),
            scrim: rgba(0x1E, 0x1E, 0x32, 0.22),
        }
    }
}

/// Syntax highlighting colors.
#[derive(Debug, Clone, Copy)]
pub struct Syntax {
    pub keyword: Color,
    pub string: Color,
    pub function: Color,
    pub number: Color,
    pub comment: Color,
    pub type_: Color,
}

pub fn syntax(dark: bool) -> Syntax {
    if dark {
        Syntax {
            keyword: rgb(0xFF, 0x9B, 0x6B),
            string: rgb(0x9B, 0xE3, 0xA4),
            function: rgb(0x7C, 0xB6, 0xFF),
            number: rgb(0xF5, 0xD0, 0x7A),
            comment: rgb(0x78, 0x80, 0x94),
            type_: rgb(0xC4, 0xA6, 0xFF),
        }
    } else {
        Syntax {
            keyword: rgb(0xC5, 0x51, 0x1D),
            string: rgb(0x1D, 0x87, 0x49),
            function: rgb(0x25, 0x63, 0xC9),
            number: rgb(0x98, 0x6E, 0x0A),
            comment: rgb(0x70, 0x76, 0x84),
            type_: rgb(0x7A, 0x4F, 0xD0),
        }
    }
}

fn border(color: Color, width: f32, radius: f32) -> Border {
    Border {
        color,
        width,
        radius: radius.into(),
    }
}

fn no_border() -> Border {
    border(Color::TRANSPARENT, 0.0, 0.0)
}

/// Shadow of the floating elements (search card, menus, palette).
fn float_shadow(dark: bool) -> Shadow {
    if dark {
        Shadow {
            color: rgba(0, 0, 0, 0.5),
            offset: Vector::new(0.0, 18.0),
            blur_radius: 50.0,
        }
    } else {
        Shadow {
            color: rgba(30, 30, 50, 0.14),
            offset: Vector::new(0.0, 14.0),
            blur_radius: 40.0,
        }
    }
}

/// A flat, transparent container (title bar, toolbar, status bar).
pub fn bar(dark: bool) -> impl Fn(&Theme) -> container::Style {
    let p = palette(dark);
    move |_theme: &Theme| container::Style {
        text_color: Some(p.text),
        background: None,
        border: no_border(),
        shadow: Shadow::default(),
        ..container::Style::default()
    }
}

/// The window background behind the floating panels.
pub fn window(dark: bool) -> impl Fn(&Theme) -> container::Style {
    let p = palette(dark);
    move |_theme: &Theme| container::Style {
        text_color: Some(p.text),
        background: Some(Background::Color(p.bg)),
        border: no_border(),
        shadow: Shadow::default(),
        ..container::Style::default()
    }
}

/// The sidebar panel: a rounded, bordered surface.
pub fn panel(dark: bool) -> impl Fn(&Theme) -> container::Style {
    let p = palette(dark);
    move |_theme: &Theme| container::Style {
        text_color: Some(p.text),
        background: Some(Background::Color(p.surface)),
        border: border(p.border, 1.0, R_PANEL),
        shadow: Shadow::default(),
        ..container::Style::default()
    }
}

/// The editor panel.
pub fn editor_panel(dark: bool) -> impl Fn(&Theme) -> container::Style {
    let p = palette(dark);
    move |_theme: &Theme| container::Style {
        text_color: Some(p.text),
        background: Some(Background::Color(p.panel)),
        border: border(p.border, 1.0, R_PANEL),
        shadow: Shadow::default(),
        ..container::Style::default()
    }
}

/// A settings card: rows on the panel color, separated by hairlines.
pub fn settings_card(dark: bool) -> impl Fn(&Theme) -> container::Style {
    editor_panel(dark)
}

/// A floating card (dropdown menus, context menu, search card, dialogs).
pub fn card(dark: bool) -> impl Fn(&Theme) -> container::Style {
    let p = palette(dark);
    move |_theme: &Theme| container::Style {
        text_color: Some(p.text),
        background: Some(Background::Color(p.elevated)),
        border: border(p.border_strong, 1.0, R_PANEL),
        shadow: float_shadow(dark),
        ..container::Style::default()
    }
}

/// The command palette box.
pub fn palette_card(dark: bool) -> impl Fn(&Theme) -> container::Style {
    let p = palette(dark);
    move |_theme: &Theme| container::Style {
        text_color: Some(p.text),
        background: Some(Background::Color(p.elevated)),
        border: border(p.border_strong, 1.0, R_PALETTE),
        shadow: float_shadow(dark),
        ..container::Style::default()
    }
}

/// A tooltip bubble.
pub fn tooltip(dark: bool) -> impl Fn(&Theme) -> container::Style {
    let p = palette(dark);
    move |_theme: &Theme| container::Style {
        text_color: Some(p.text),
        background: Some(Background::Color(p.elevated)),
        border: border(p.border_strong, 1.0, R_SM),
        shadow: Shadow {
            color: rgba(0, 0, 0, 0.3),
            offset: Vector::new(0.0, 4.0),
            blur_radius: 12.0,
        },
        ..container::Style::default()
    }
}

/// A tiny bordered badge (the `esc` hint of the palette).
pub fn badge(dark: bool) -> impl Fn(&Theme) -> container::Style {
    let p = palette(dark);
    move |_theme: &Theme| container::Style {
        text_color: Some(p.muted),
        background: None,
        border: border(p.border, 1.0, 5.0),
        shadow: Shadow::default(),
        ..container::Style::default()
    }
}

/// A small filled circle (state dots), sized by the container.
pub fn dot(color: Color) -> impl Fn(&Theme) -> container::Style {
    move |_theme: &Theme| container::Style {
        text_color: None,
        background: Some(Background::Color(color)),
        border: border(Color::TRANSPARENT, 0.0, R_PILL),
        shadow: Shadow::default(),
        ..container::Style::default()
    }
}

/// A 1px rule (set the thickness with `Space`).
pub fn rule(dark: bool) -> impl Fn(&Theme) -> container::Style {
    let p = palette(dark);
    move |_theme: &Theme| container::Style {
        text_color: None,
        background: Some(Background::Color(p.border)),
        border: no_border(),
        shadow: Shadow::default(),
        ..container::Style::default()
    }
}

/// A rounded field (inputs, selects, segmented controls) on the window
/// background, with a hairline or an accent (focus) border.
pub fn field(dark: bool, focused: bool, radius: f32) -> impl Fn(&Theme) -> container::Style {
    let p = palette(dark);
    move |_theme: &Theme| container::Style {
        text_color: Some(p.text),
        background: Some(Background::Color(p.bg)),
        border: border(if focused { p.accent } else { p.border }, 1.0, radius),
        shadow: Shadow::default(),
        ..container::Style::default()
    }
}

/// A muted "eyebrow" label color (section headers like FICHIERS).
pub fn muted_text(dark: bool) -> Color {
    palette(dark).muted
}

pub fn accent_color(dark: bool) -> Color {
    palette(dark).accent
}

/// Bottom error panel (shown when a formatter returns an error).
pub fn error_panel(dark: bool) -> impl Fn(&Theme) -> container::Style {
    let error = error_color(dark);
    move |_theme: &Theme| container::Style {
        text_color: Some(error),
        background: None,
        border: no_border(),
        shadow: Shadow::default(),
        ..container::Style::default()
    }
}

pub fn error_color(dark: bool) -> Color {
    if dark {
        rgb(0xFF, 0x6B, 0x6B)
    } else {
        rgb(0xC0, 0x42, 0x2D)
    }
}

/// Accent-tinted banner (shown during async operations like formatting).
pub fn accent_banner(dark: bool) -> impl Fn(&Theme) -> container::Style {
    let p = palette(dark);
    move |_theme: &Theme| container::Style {
        text_color: Some(p.accent),
        background: Some(Background::Color(p.accent_soft)),
        border: no_border(),
        shadow: Shadow::default(),
        ..container::Style::default()
    }
}

/// Fenced code block inside the Markdown preview.
pub fn code_block(dark: bool) -> impl Fn(&Theme) -> container::Style {
    let p = palette(dark);
    move |_theme: &Theme| container::Style {
        text_color: Some(p.text),
        background: Some(Background::Color(p.bg)),
        border: border(p.border, 1.0, R_ROW),
        shadow: Shadow::default(),
        ..container::Style::default()
    }
}

/// Block quote inside the Markdown preview: a soft accent callout.
pub fn quote_block(dark: bool) -> impl Fn(&Theme) -> container::Style {
    let p = palette(dark);
    move |_theme: &Theme| container::Style {
        text_color: Some(p.text),
        background: Some(Background::Color(p.accent_soft)),
        border: border(Color::TRANSPARENT, 0.0, R_ROW),
        shadow: Shadow::default(),
        ..container::Style::default()
    }
}

/// A table's outer frame in the Markdown preview: a plain 1px outline, no
/// fill. The header/body divider is the same hairline, drawn separately.
pub fn table_border(dark: bool) -> impl Fn(&Theme) -> container::Style {
    let p = palette(dark);
    move |_theme: &Theme| container::Style {
        text_color: Some(p.text),
        background: None,
        border: border(p.border, 1.0, R_SM),
        shadow: Shadow::default(),
        ..container::Style::default()
    }
}

fn button_appearance(background: Option<Color>, text: Color, border: Border) -> button::Style {
    button::Style {
        background: background.map(Background::Color),
        text_color: text,
        border,
        shadow: Shadow::default(),
        snap: false,
    }
}

/// A disabled button: its active look, faded.
fn dimmed(mut style: button::Style) -> button::Style {
    style.background = style.background.map(|b| match b {
        Background::Color(c) => Background::Color(Color { a: c.a * 0.5, ..c }),
        other => other,
    });
    style.text_color.a *= 0.5;
    style.border.color.a *= 0.5;
    style
}

/// Ghost button used in toolbars and menu items: transparent, a soft accent
/// wash when active/open, a subtle tint on hover.
pub struct GhostButton {
    pub dark: bool,
    pub active: bool,
}

impl GhostButton {
    fn look(&self, hovered: bool) -> button::Style {
        let p = palette(self.dark);
        let background = if self.active {
            Some(p.accent_soft)
        } else if hovered {
            Some(p.hover)
        } else {
            None
        };
        button_appearance(
            background,
            if self.active { p.accent } else { p.text },
            border(Color::TRANSPARENT, 0.0, R_ROW),
        )
    }
}

impl GhostButton {
    pub fn style(self) -> impl Fn(&Theme, button::Status) -> button::Style {
        move |_theme, status| match status {
            button::Status::Active => self.active(),
            button::Status::Hovered => self.hovered(),
            button::Status::Pressed => self.pressed(),
            button::Status::Disabled => dimmed(self.active()),
        }
    }

    fn active(&self) -> button::Style {
        self.look(false)
    }

    fn hovered(&self) -> button::Style {
        self.look(true)
    }

    fn pressed(&self) -> button::Style {
        self.hovered()
    }
}

/// A menu entry (dropdown, context menu, palette row): small radius; the
/// hovered or highlighted one gets the soft accent wash and accent text.
pub struct MenuItem {
    pub dark: bool,
    pub radius: f32,
    pub highlighted: bool,
}

impl MenuItem {
    pub fn style(self) -> impl Fn(&Theme, button::Status) -> button::Style {
        move |_theme, status| match status {
            button::Status::Active => self.active(),
            button::Status::Hovered => self.hovered(),
            button::Status::Pressed => self.pressed(),
            button::Status::Disabled => dimmed(self.active()),
        }
    }

    fn active(&self) -> button::Style {
        let p = palette(self.dark);
        button_appearance(
            self.highlighted.then_some(p.accent_soft),
            if self.highlighted { p.accent } else { p.text },
            border(Color::TRANSPARENT, 0.0, self.radius),
        )
    }

    fn hovered(&self) -> button::Style {
        let p = palette(self.dark);
        button_appearance(
            Some(p.accent_soft),
            p.accent,
            border(Color::TRANSPARENT, 0.0, self.radius),
        )
    }

    fn pressed(&self) -> button::Style {
        self.hovered()
    }
}

/// A title of the menu bar (File, Edit…): muted, small radius.
pub struct MenuTitle {
    pub dark: bool,
    pub open: bool,
}

impl MenuTitle {
    pub fn style(self) -> impl Fn(&Theme, button::Status) -> button::Style {
        move |_theme, status| match status {
            button::Status::Active => self.active(),
            button::Status::Hovered => self.hovered(),
            button::Status::Pressed => self.pressed(),
            button::Status::Disabled => dimmed(self.active()),
        }
    }

    fn active(&self) -> button::Style {
        let p = palette(self.dark);
        button_appearance(
            self.open.then_some(p.accent_soft),
            if self.open { p.accent } else { p.muted },
            border(Color::TRANSPARENT, 0.0, R_MENU_TITLE),
        )
    }

    fn hovered(&self) -> button::Style {
        let p = palette(self.dark);
        button_appearance(
            Some(if self.open { p.accent_soft } else { p.hover }),
            if self.open { p.accent } else { p.text },
            border(Color::TRANSPARENT, 0.0, R_MENU_TITLE),
        )
    }

    fn pressed(&self) -> button::Style {
        self.hovered()
    }
}

/// A segment of a segmented control (Brut/Aperçu/Divisé, 2/4/8, Sombre/Clair):
/// the active one is an accent wash.
pub struct Segment {
    pub dark: bool,
    pub active: bool,
    pub radius: f32,
}

impl Segment {
    pub fn style(self) -> impl Fn(&Theme, button::Status) -> button::Style {
        move |_theme, status| match status {
            button::Status::Active => self.active(),
            button::Status::Hovered => self.hovered(),
            button::Status::Pressed => self.pressed(),
            button::Status::Disabled => dimmed(self.active()),
        }
    }

    fn active(&self) -> button::Style {
        let p = palette(self.dark);
        button_appearance(
            self.active.then_some(p.accent_soft),
            if self.active { p.accent } else { p.muted },
            border(Color::TRANSPARENT, 0.0, self.radius),
        )
    }

    fn hovered(&self) -> button::Style {
        let p = palette(self.dark);
        button_appearance(
            Some(if self.active { p.accent_soft } else { p.hover }),
            if self.active { p.accent } else { p.text },
            border(Color::TRANSPARENT, 0.0, self.radius),
        )
    }

    fn pressed(&self) -> button::Style {
        self.hovered()
    }
}

/// A small toggle chip (`.*`, `Aa`).
pub struct Chip {
    pub dark: bool,
    pub active: bool,
}

impl Chip {
    pub fn style(self) -> impl Fn(&Theme, button::Status) -> button::Style {
        move |_theme, status| match status {
            button::Status::Active => self.active(),
            button::Status::Hovered => self.hovered(),
            button::Status::Pressed => self.pressed(),
            button::Status::Disabled => dimmed(self.active()),
        }
    }

    fn active(&self) -> button::Style {
        let p = palette(self.dark);
        button_appearance(
            self.active.then_some(p.accent_soft),
            if self.active { p.accent } else { p.muted },
            border(Color::TRANSPARENT, 0.0, R_SM),
        )
    }

    fn hovered(&self) -> button::Style {
        let p = palette(self.dark);
        button_appearance(
            Some(if self.active { p.accent_soft } else { p.hover }),
            if self.active { p.accent } else { p.text },
            border(Color::TRANSPARENT, 0.0, R_SM),
        )
    }

    fn pressed(&self) -> button::Style {
        self.hovered()
    }
}

/// A button outlined by a hairline (Formater le code, Remplacer).
pub struct Outline {
    pub dark: bool,
}

impl Outline {
    pub fn style(self) -> impl Fn(&Theme, button::Status) -> button::Style {
        move |_theme, status| match status {
            button::Status::Active => self.active(),
            button::Status::Hovered => self.hovered(),
            button::Status::Pressed => self.pressed(),
            button::Status::Disabled => self.disabled(),
        }
    }

    fn active(&self) -> button::Style {
        let p = palette(self.dark);
        button_appearance(None, p.text, border(p.border, 1.0, R_ROW))
    }

    fn hovered(&self) -> button::Style {
        let p = palette(self.dark);
        button_appearance(Some(p.hover), p.text, border(p.border_strong, 1.0, R_ROW))
    }

    fn pressed(&self) -> button::Style {
        self.hovered()
    }

    fn disabled(&self) -> button::Style {
        let p = palette(self.dark);
        button_appearance(None, p.muted, border(p.border, 1.0, R_ROW))
    }
}

/// The primary action: accent fill, dark ink.
pub struct Primary {
    pub dark: bool,
}

impl Primary {
    pub fn style(self) -> impl Fn(&Theme, button::Status) -> button::Style {
        move |_theme, status| match status {
            button::Status::Active => self.active(),
            button::Status::Hovered => self.hovered(),
            button::Status::Pressed => self.pressed(),
            button::Status::Disabled => dimmed(self.active()),
        }
    }

    fn active(&self) -> button::Style {
        let p = palette(self.dark);
        button_appearance(
            Some(p.accent),
            p.accent_ink,
            border(Color::TRANSPARENT, 0.0, R_ROW),
        )
    }

    fn hovered(&self) -> button::Style {
        let p = palette(self.dark);
        button_appearance(
            Some(Color {
                a: 0.88,
                ..p.accent
            }),
            p.accent_ink,
            border(Color::TRANSPARENT, 0.0, R_ROW),
        )
    }

    fn pressed(&self) -> button::Style {
        self.hovered()
    }
}

/// Sidebar file-row button: selected rows get a soft accent wash.
pub struct SidebarRow {
    pub dark: bool,
    pub selected: bool,
}

impl SidebarRow {
    fn look(&self, hovered: bool) -> button::Style {
        let p = palette(self.dark);
        let background = if self.selected {
            Some(p.accent_soft)
        } else if hovered {
            Some(p.hover)
        } else {
            None
        };
        button_appearance(
            background,
            if self.selected { p.accent } else { p.text },
            border(Color::TRANSPARENT, 0.0, R_ROW),
        )
    }
}

impl SidebarRow {
    pub fn style(self) -> impl Fn(&Theme, button::Status) -> button::Style {
        move |_theme, status| match status {
            button::Status::Active => self.active(),
            button::Status::Hovered => self.hovered(),
            button::Status::Pressed => self.pressed(),
            button::Status::Disabled => dimmed(self.active()),
        }
    }

    fn active(&self) -> button::Style {
        self.look(false)
    }

    fn hovered(&self) -> button::Style {
        self.look(true)
    }

    fn pressed(&self) -> button::Style {
        self.hovered()
    }
}

/// Switch (toggle) button; the knob is laid out by the caller, see
/// `switch` in `preferences`.
pub struct SwitchTrack {
    pub dark: bool,
    pub on: bool,
}

impl SwitchTrack {
    pub fn style(self) -> impl Fn(&Theme, button::Status) -> button::Style {
        move |_theme, status| match status {
            button::Status::Active => self.active(),
            button::Status::Hovered => self.hovered(),
            button::Status::Pressed => self.pressed(),
            button::Status::Disabled => dimmed(self.active()),
        }
    }

    fn active(&self) -> button::Style {
        let p = palette(self.dark);
        if self.on {
            button_appearance(Some(p.accent), p.accent_ink, border(p.accent, 1.0, 10.0))
        } else {
            button_appearance(Some(p.bg), p.muted, border(p.border_strong, 1.0, 10.0))
        }
    }

    fn hovered(&self) -> button::Style {
        self.active()
    }

    fn pressed(&self) -> button::Style {
        self.active()
    }
}

/// The knob of a switch.
pub fn switch_knob(dark: bool, on: bool) -> impl Fn(&Theme) -> container::Style {
    let p = palette(dark);
    move |_theme: &Theme| container::Style {
        text_color: None,
        background: Some(Background::Color(if on { p.accent_ink } else { p.muted })),
        border: border(Color::TRANSPARENT, 0.0, 7.0),
        shadow: Shadow::default(),
        ..container::Style::default()
    }
}

/// Text input: a rounded field on the window background; accent border when
/// focused (or always, for the primary search field).
pub struct Field {
    pub dark: bool,
    pub accent: bool,
    pub radius: f32,
}

impl Field {
    pub fn new(dark: bool) -> Self {
        Self {
            dark,
            accent: false,
            radius: R_ROW,
        }
    }
}

impl Field {
    pub fn style(self) -> impl Fn(&Theme, text_input::Status) -> text_input::Style {
        move |_theme, status| {
            let p = palette(self.dark);
            let border = match status {
                text_input::Status::Focused { .. } => border(p.accent, 1.0, self.radius),
                _ => border(
                    if self.accent { p.accent } else { p.border },
                    1.0,
                    self.radius,
                ),
            };
            text_input::Style {
                background: Background::Color(p.bg),
                border,
                icon: p.muted,
                placeholder: p.muted,
                value: p.text,
                selection: input_selection(self.dark),
            }
        }
    }
}

/// `accent_soft` (a 10-14% wash) is all but invisible as a selection.
fn input_selection(dark: bool) -> Color {
    Color {
        a: 0.28,
        ..palette(dark).accent
    }
}

/// A bare text input, for embedding in a container that draws the field.
pub struct BareField {
    pub dark: bool,
}

impl BareField {
    pub fn style(self) -> impl Fn(&Theme, text_input::Status) -> text_input::Style {
        move |_theme, _status| {
            let p = palette(self.dark);
            text_input::Style {
                background: Background::Color(Color::TRANSPARENT),
                border: no_border(),
                icon: p.muted,
                placeholder: p.muted,
                value: p.text,
                selection: input_selection(self.dark),
            }
        }
    }
}

/// Slider: a 4px rail filled with the accent, a round light handle.
pub struct Slider {
    pub dark: bool,
}

impl Slider {
    pub fn style(self) -> impl Fn(&Theme, slider::Status) -> slider::Style {
        move |_theme, status| {
            let p = palette(self.dark);
            let handle = match status {
                slider::Status::Dragged => p.accent,
                _ => p.text,
            };
            slider::Style {
                rail: slider::Rail {
                    backgrounds: (
                        Background::Color(p.accent),
                        Background::Color(p.border_strong),
                    ),
                    width: 4.0,
                    border: border(Color::TRANSPARENT, 0.0, 2.0),
                },
                handle: slider::Handle {
                    shape: slider::HandleShape::Circle { radius: 7.0 },
                    background: Background::Color(handle),
                    border_width: 0.0,
                    border_color: Color::TRANSPARENT,
                },
            }
        }
    }
}

/// Select (pick list): a field with a chevron.
pub struct Select {
    pub dark: bool,
}

impl Select {
    pub fn style(self) -> impl Fn(&Theme, pick_list::Status) -> pick_list::Style {
        move |_theme, status| {
            let p = palette(self.dark);
            let line = match status {
                pick_list::Status::Active => p.border,
                _ => p.border_strong,
            };
            pick_list::Style {
                text_color: p.text,
                placeholder_color: p.muted,
                handle_color: p.muted,
                background: Background::Color(p.bg),
                border: border(line, 1.0, R_ROW),
            }
        }
    }

    pub fn menu_style(self) -> impl Fn(&Theme) -> iced::overlay::menu::Style {
        move |_theme| {
            let p = palette(self.dark);
            iced::overlay::menu::Style {
                text_color: p.text,
                background: Background::Color(p.elevated),
                border: border(p.border_strong, 1.0, R_ROW),
                selected_text_color: p.accent,
                selected_background: Background::Color(p.accent_soft),
                shadow: Shadow::default(),
            }
        }
    }
}

/// The dimmed backdrop behind modal dialogs and the palette.
pub fn scrim(dark: bool) -> impl Fn(&Theme) -> container::Style {
    let p = palette(dark);
    move |_theme| container::Style {
        background: Some(Background::Color(p.scrim)),
        ..container::Style::default()
    }
}

/// Slim scrollbars that stay out of the way.
pub struct Scrollbar {
    pub dark: bool,
}

impl Scrollbar {
    pub fn style(self) -> impl Fn(&Theme, scrollable::Status) -> scrollable::Style {
        move |theme, status| {
            let p = palette(self.dark);
            let strong = matches!(
                status,
                scrollable::Status::Dragged { .. }
                    | scrollable::Status::Hovered {
                        is_vertical_scrollbar_hovered: true,
                        ..
                    }
            );
            let rail = scrollable::Rail {
                background: None,
                border: no_border(),
                scroller: scrollable::Scroller {
                    background: Background::Color(if strong { p.muted } else { p.border_strong }),
                    border: border(Color::TRANSPARENT, 0.0, 4.0),
                },
            };
            scrollable::Style {
                container: container::Style::default(),
                vertical_rail: rail,
                horizontal_rail: rail,
                gap: None,
                ..scrollable::default(theme, status)
            }
        }
    }
}

/// The code editor: transparent (the panel behind it provides the surface),
/// accent caret and selection.
pub fn editor_style(dark: bool) -> crate::code_editor::Style {
    let p = palette(dark);
    crate::code_editor::Style {
        background: Background::Color(Color::TRANSPARENT),
        border: no_border(),
        value: p.accent,
        selection: Color {
            a: 0.28,
            ..p.accent
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WCAG contrast ratio of two opaque colors.
    fn contrast(a: Color, b: Color) -> f32 {
        let channel = |c: f32| {
            if c <= 0.03928 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        let luminance =
            |c: Color| 0.2126 * channel(c.r) + 0.7152 * channel(c.g) + 0.0722 * channel(c.b);
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    #[test]
    fn code_colors_are_readable_on_the_editor_panel() {
        for dark in [true, false] {
            let (p, s) = (palette(dark), syntax(dark));
            for (name, color) in [
                ("keyword", s.keyword),
                ("string", s.string),
                ("function", s.function),
                ("number", s.number),
                ("comment", s.comment),
                ("type", s.type_),
            ] {
                assert!(contrast(color, p.panel) >= 4.5, "{name} (dark: {dark})");
            }
            // Line numbers are decorative: WCAG asks 3:1 for those.
            assert!(contrast(p.gutter, p.panel) >= 3.0, "gutter (dark: {dark})");
        }
    }

    #[test]
    fn palettes_follow_the_design_tokens() {
        let d = palette(true);
        assert_eq!(d.bg, rgb(0x07, 0x09, 0x0E));
        assert_eq!(d.accent, ACCENT_DARK);
        let l = palette(false);
        assert_eq!(l.panel, rgb(0xFF, 0xFF, 0xFF));
        assert_eq!(l.accent, ACCENT_LIGHT);
        assert_ne!(d.accent, l.accent);
    }

    #[test]
    fn syntax_colors_differ_between_modes() {
        assert_ne!(syntax(true).keyword, syntax(false).keyword);
        assert_eq!(syntax(true).keyword, palette(true).accent);
    }

    #[test]
    fn every_embedded_font_is_a_plausible_truetype_file() {
        // Only the size and the TrueType header are checked (not the name
        // table): a truncated or wrong file would silently fall back to a
        // system font.
        for bytes in font_files() {
            assert!(bytes.len() > 10_000);
            assert_eq!(&bytes[..4], &[0, 1, 0, 0], "not a TrueType file");
        }
    }
}
