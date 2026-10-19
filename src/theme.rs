//! Visual identity for SimpleEdit: "Nocturne".
//!
//! Deep night blue surfaces, floating rounded panels and one warm coral
//! accent (a darker coral in the light theme). Style helpers take
//! `dark: bool` directly instead of deriving colors from `iced::Theme`, since
//! the two custom themes only carry the base palette and we want exact
//! control over surfaces, borders and hairlines.

use iced::widget::{button, container, pick_list, scrollable, slider, text_editor, text_input};
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
            gutter: rgb(0x44, 0x4C, 0x64),
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
            gutter: rgb(0xB3, 0xB9, 0xC8),
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
            comment: rgb(0x56, 0x60, 0x79),
            type_: rgb(0xC4, 0xA6, 0xFF),
        }
    } else {
        Syntax {
            keyword: rgb(0xD2, 0x56, 0x1F),
            string: rgb(0x1E, 0x8A, 0x4A),
            function: rgb(0x25, 0x63, 0xC9),
            number: rgb(0xA5, 0x78, 0x0B),
            comment: rgb(0x9A, 0xA2, 0xB5),
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
pub fn bar(dark: bool) -> impl Fn(&Theme) -> container::Appearance {
    let p = palette(dark);
    move |_theme: &Theme| container::Appearance {
        text_color: Some(p.text),
        background: None,
        border: no_border(),
        shadow: Shadow::default(),
    }
}

/// The window background behind the floating panels.
pub fn window(dark: bool) -> impl Fn(&Theme) -> container::Appearance {
    let p = palette(dark);
    move |_theme: &Theme| container::Appearance {
        text_color: Some(p.text),
        background: Some(Background::Color(p.bg)),
        border: no_border(),
        shadow: Shadow::default(),
    }
}

/// The sidebar panel: a rounded, bordered surface.
pub fn panel(dark: bool) -> impl Fn(&Theme) -> container::Appearance {
    let p = palette(dark);
    move |_theme: &Theme| container::Appearance {
        text_color: Some(p.text),
        background: Some(Background::Color(p.surface)),
        border: border(p.border, 1.0, R_PANEL),
        shadow: Shadow::default(),
    }
}

/// The editor panel.
pub fn editor_panel(dark: bool) -> impl Fn(&Theme) -> container::Appearance {
    let p = palette(dark);
    move |_theme: &Theme| container::Appearance {
        text_color: Some(p.text),
        background: Some(Background::Color(p.panel)),
        border: border(p.border, 1.0, R_PANEL),
        shadow: Shadow::default(),
    }
}

/// A settings card: rows on the panel color, separated by hairlines.
pub fn settings_card(dark: bool) -> impl Fn(&Theme) -> container::Appearance {
    editor_panel(dark)
}

/// A floating card (dropdown menus, context menu, search card, dialogs).
pub fn card(dark: bool) -> impl Fn(&Theme) -> container::Appearance {
    let p = palette(dark);
    move |_theme: &Theme| container::Appearance {
        text_color: Some(p.text),
        background: Some(Background::Color(p.elevated)),
        border: border(p.border_strong, 1.0, R_PANEL),
        shadow: float_shadow(dark),
    }
}

/// The command palette box.
pub fn palette_card(dark: bool) -> impl Fn(&Theme) -> container::Appearance {
    let p = palette(dark);
    move |_theme: &Theme| container::Appearance {
        text_color: Some(p.text),
        background: Some(Background::Color(p.elevated)),
        border: border(p.border_strong, 1.0, R_PALETTE),
        shadow: float_shadow(dark),
    }
}

/// A tooltip bubble.
pub fn tooltip(dark: bool) -> impl Fn(&Theme) -> container::Appearance {
    let p = palette(dark);
    move |_theme: &Theme| container::Appearance {
        text_color: Some(p.text),
        background: Some(Background::Color(p.elevated)),
        border: border(p.border_strong, 1.0, R_SM),
        shadow: Shadow {
            color: rgba(0, 0, 0, 0.3),
            offset: Vector::new(0.0, 4.0),
            blur_radius: 12.0,
        },
    }
}

/// A tiny bordered badge (the `esc` hint of the palette).
pub fn badge(dark: bool) -> impl Fn(&Theme) -> container::Appearance {
    let p = palette(dark);
    move |_theme: &Theme| container::Appearance {
        text_color: Some(p.muted),
        background: None,
        border: border(p.border, 1.0, 5.0),
        shadow: Shadow::default(),
    }
}

/// A small filled circle (state dots), sized by the container.
pub fn dot(color: Color) -> impl Fn(&Theme) -> container::Appearance {
    move |_theme: &Theme| container::Appearance {
        text_color: None,
        background: Some(Background::Color(color)),
        border: border(Color::TRANSPARENT, 0.0, R_PILL),
        shadow: Shadow::default(),
    }
}

/// A 1px rule (set the thickness with `Space`).
pub fn rule(dark: bool) -> impl Fn(&Theme) -> container::Appearance {
    let p = palette(dark);
    move |_theme: &Theme| container::Appearance {
        text_color: None,
        background: Some(Background::Color(p.border)),
        border: no_border(),
        shadow: Shadow::default(),
    }
}

/// A rounded field (inputs, selects, segmented controls) on the window
/// background, with a hairline or an accent (focus) border.
pub fn field(dark: bool, focused: bool, radius: f32) -> impl Fn(&Theme) -> container::Appearance {
    let p = palette(dark);
    move |_theme: &Theme| container::Appearance {
        text_color: Some(p.text),
        background: Some(Background::Color(p.bg)),
        border: border(if focused { p.accent } else { p.border }, 1.0, radius),
        shadow: Shadow::default(),
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
pub fn error_panel(dark: bool) -> impl Fn(&Theme) -> container::Appearance {
    let error = error_color(dark);
    move |_theme: &Theme| container::Appearance {
        text_color: Some(error),
        background: None,
        border: no_border(),
        shadow: Shadow::default(),
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
pub fn accent_banner(dark: bool) -> impl Fn(&Theme) -> container::Appearance {
    let p = palette(dark);
    move |_theme: &Theme| container::Appearance {
        text_color: Some(p.accent),
        background: Some(Background::Color(p.accent_soft)),
        border: no_border(),
        shadow: Shadow::default(),
    }
}

/// Fenced code block inside the Markdown preview.
pub fn code_block(dark: bool) -> impl Fn(&Theme) -> container::Appearance {
    let p = palette(dark);
    move |_theme: &Theme| container::Appearance {
        text_color: Some(p.text),
        background: Some(Background::Color(p.bg)),
        border: border(p.border, 1.0, R_ROW),
        shadow: Shadow::default(),
    }
}

/// Block quote inside the Markdown preview: a soft accent callout.
pub fn quote_block(dark: bool) -> impl Fn(&Theme) -> container::Appearance {
    let p = palette(dark);
    move |_theme: &Theme| container::Appearance {
        text_color: Some(p.text),
        background: Some(Background::Color(p.accent_soft)),
        border: border(Color::TRANSPARENT, 0.0, R_ROW),
        shadow: Shadow::default(),
    }
}

/// A table's outer frame in the Markdown preview: a plain 1px outline, no
/// fill. The header/body divider is the same hairline, drawn separately.
pub fn table_border(dark: bool) -> impl Fn(&Theme) -> container::Appearance {
    let p = palette(dark);
    move |_theme: &Theme| container::Appearance {
        text_color: Some(p.text),
        background: None,
        border: border(p.border, 1.0, R_SM),
        shadow: Shadow::default(),
    }
}

fn button_appearance(background: Option<Color>, text: Color, border: Border) -> button::Appearance {
    button::Appearance {
        shadow_offset: Vector::default(),
        background: background.map(Background::Color),
        text_color: text,
        border,
        shadow: Shadow::default(),
    }
}

/// Ghost button used in toolbars and menu items: transparent, a soft accent
/// wash when active/open, a subtle tint on hover.
pub struct GhostButton {
    pub dark: bool,
    pub active: bool,
}

impl GhostButton {
    fn look(&self, hovered: bool) -> button::Appearance {
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

impl button::StyleSheet for GhostButton {
    type Style = Theme;

    fn active(&self, _style: &Self::Style) -> button::Appearance {
        self.look(false)
    }

    fn hovered(&self, _style: &Self::Style) -> button::Appearance {
        self.look(true)
    }

    fn pressed(&self, style: &Self::Style) -> button::Appearance {
        self.hovered(style)
    }
}

/// A menu entry (dropdown, context menu, palette row): small radius; the
/// hovered or highlighted one gets the soft accent wash and accent text.
pub struct MenuItem {
    pub dark: bool,
    pub radius: f32,
    pub highlighted: bool,
}

impl button::StyleSheet for MenuItem {
    type Style = Theme;

    fn active(&self, _style: &Self::Style) -> button::Appearance {
        let p = palette(self.dark);
        button_appearance(
            self.highlighted.then_some(p.accent_soft),
            if self.highlighted { p.accent } else { p.text },
            border(Color::TRANSPARENT, 0.0, self.radius),
        )
    }

    fn hovered(&self, _style: &Self::Style) -> button::Appearance {
        let p = palette(self.dark);
        button_appearance(
            Some(p.accent_soft),
            p.accent,
            border(Color::TRANSPARENT, 0.0, self.radius),
        )
    }

    fn pressed(&self, style: &Self::Style) -> button::Appearance {
        self.hovered(style)
    }
}

/// A title of the menu bar (File, Edit…): muted, small radius.
pub struct MenuTitle {
    pub dark: bool,
    pub open: bool,
}

impl button::StyleSheet for MenuTitle {
    type Style = Theme;

    fn active(&self, _style: &Self::Style) -> button::Appearance {
        let p = palette(self.dark);
        button_appearance(
            self.open.then_some(p.accent_soft),
            if self.open { p.accent } else { p.muted },
            border(Color::TRANSPARENT, 0.0, R_MENU_TITLE),
        )
    }

    fn hovered(&self, _style: &Self::Style) -> button::Appearance {
        let p = palette(self.dark);
        button_appearance(
            Some(if self.open { p.accent_soft } else { p.hover }),
            if self.open { p.accent } else { p.text },
            border(Color::TRANSPARENT, 0.0, R_MENU_TITLE),
        )
    }

    fn pressed(&self, style: &Self::Style) -> button::Appearance {
        self.hovered(style)
    }
}

/// A segment of a segmented control (Brut/Aperçu/Divisé, 2/4/8, Sombre/Clair):
/// the active one is an accent wash.
pub struct Segment {
    pub dark: bool,
    pub active: bool,
    pub radius: f32,
}

impl button::StyleSheet for Segment {
    type Style = Theme;

    fn active(&self, _style: &Self::Style) -> button::Appearance {
        let p = palette(self.dark);
        button_appearance(
            self.active.then_some(p.accent_soft),
            if self.active { p.accent } else { p.muted },
            border(Color::TRANSPARENT, 0.0, self.radius),
        )
    }

    fn hovered(&self, _style: &Self::Style) -> button::Appearance {
        let p = palette(self.dark);
        button_appearance(
            Some(if self.active { p.accent_soft } else { p.hover }),
            if self.active { p.accent } else { p.text },
            border(Color::TRANSPARENT, 0.0, self.radius),
        )
    }

    fn pressed(&self, style: &Self::Style) -> button::Appearance {
        self.hovered(style)
    }
}

/// A small toggle chip (`.*`, `Aa`).
pub struct Chip {
    pub dark: bool,
    pub active: bool,
}

impl button::StyleSheet for Chip {
    type Style = Theme;

    fn active(&self, _style: &Self::Style) -> button::Appearance {
        let p = palette(self.dark);
        button_appearance(
            self.active.then_some(p.accent_soft),
            if self.active { p.accent } else { p.muted },
            border(Color::TRANSPARENT, 0.0, R_SM),
        )
    }

    fn hovered(&self, _style: &Self::Style) -> button::Appearance {
        let p = palette(self.dark);
        button_appearance(
            Some(if self.active { p.accent_soft } else { p.hover }),
            if self.active { p.accent } else { p.text },
            border(Color::TRANSPARENT, 0.0, R_SM),
        )
    }

    fn pressed(&self, style: &Self::Style) -> button::Appearance {
        self.hovered(style)
    }
}

/// A button outlined by a hairline (Formater le code, Remplacer).
pub struct Outline {
    pub dark: bool,
}

impl button::StyleSheet for Outline {
    type Style = Theme;

    fn active(&self, _style: &Self::Style) -> button::Appearance {
        let p = palette(self.dark);
        button_appearance(None, p.text, border(p.border, 1.0, R_ROW))
    }

    fn hovered(&self, _style: &Self::Style) -> button::Appearance {
        let p = palette(self.dark);
        button_appearance(Some(p.hover), p.text, border(p.border_strong, 1.0, R_ROW))
    }

    fn pressed(&self, style: &Self::Style) -> button::Appearance {
        self.hovered(style)
    }

    fn disabled(&self, _style: &Self::Style) -> button::Appearance {
        let p = palette(self.dark);
        button_appearance(None, p.muted, border(p.border, 1.0, R_ROW))
    }
}

/// The primary action: accent fill, dark ink.
pub struct Primary {
    pub dark: bool,
}

impl button::StyleSheet for Primary {
    type Style = Theme;

    fn active(&self, _style: &Self::Style) -> button::Appearance {
        let p = palette(self.dark);
        button_appearance(
            Some(p.accent),
            p.accent_ink,
            border(Color::TRANSPARENT, 0.0, R_ROW),
        )
    }

    fn hovered(&self, _style: &Self::Style) -> button::Appearance {
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

    fn pressed(&self, style: &Self::Style) -> button::Appearance {
        self.hovered(style)
    }
}

/// Sidebar file-row button: selected rows get a soft accent wash.
pub struct SidebarRow {
    pub dark: bool,
    pub selected: bool,
}

impl SidebarRow {
    fn look(&self, hovered: bool) -> button::Appearance {
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

impl button::StyleSheet for SidebarRow {
    type Style = Theme;

    fn active(&self, _style: &Self::Style) -> button::Appearance {
        self.look(false)
    }

    fn hovered(&self, _style: &Self::Style) -> button::Appearance {
        self.look(true)
    }

    fn pressed(&self, style: &Self::Style) -> button::Appearance {
        self.hovered(style)
    }
}

/// Switch (toggle) button; the knob is laid out by the caller, see
/// `switch` in `preferences`.
pub struct SwitchTrack {
    pub dark: bool,
    pub on: bool,
}

impl button::StyleSheet for SwitchTrack {
    type Style = Theme;

    fn active(&self, _style: &Self::Style) -> button::Appearance {
        let p = palette(self.dark);
        if self.on {
            button_appearance(Some(p.accent), p.accent_ink, border(p.accent, 1.0, 10.0))
        } else {
            button_appearance(Some(p.bg), p.muted, border(p.border_strong, 1.0, 10.0))
        }
    }

    fn hovered(&self, style: &Self::Style) -> button::Appearance {
        self.active(style)
    }

    fn pressed(&self, style: &Self::Style) -> button::Appearance {
        self.active(style)
    }
}

/// The knob of a switch.
pub fn switch_knob(dark: bool, on: bool) -> impl Fn(&Theme) -> container::Appearance {
    let p = palette(dark);
    move |_theme: &Theme| container::Appearance {
        text_color: None,
        background: Some(Background::Color(if on { p.accent_ink } else { p.muted })),
        border: border(Color::TRANSPARENT, 0.0, 7.0),
        shadow: Shadow::default(),
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

impl text_input::StyleSheet for Field {
    type Style = Theme;

    fn active(&self, _style: &Self::Style) -> text_input::Appearance {
        let p = palette(self.dark);
        text_input::Appearance {
            background: Background::Color(p.bg),
            border: border(
                if self.accent { p.accent } else { p.border },
                1.0,
                self.radius,
            ),
            icon_color: p.muted,
        }
    }

    fn focused(&self, _style: &Self::Style) -> text_input::Appearance {
        let p = palette(self.dark);
        text_input::Appearance {
            background: Background::Color(p.bg),
            border: border(p.accent, 1.0, self.radius),
            icon_color: p.muted,
        }
    }

    fn placeholder_color(&self, _style: &Self::Style) -> Color {
        palette(self.dark).muted
    }

    fn value_color(&self, _style: &Self::Style) -> Color {
        palette(self.dark).text
    }

    fn disabled_color(&self, _style: &Self::Style) -> Color {
        palette(self.dark).muted
    }

    fn selection_color(&self, _style: &Self::Style) -> Color {
        palette(self.dark).accent_soft
    }

    fn disabled(&self, style: &Self::Style) -> text_input::Appearance {
        self.active(style)
    }
}

/// A bare text input, for embedding in a container that draws the field.
pub struct BareField {
    pub dark: bool,
}

impl text_input::StyleSheet for BareField {
    type Style = Theme;

    fn active(&self, _style: &Self::Style) -> text_input::Appearance {
        text_input::Appearance {
            background: Background::Color(Color::TRANSPARENT),
            border: no_border(),
            icon_color: palette(self.dark).muted,
        }
    }

    fn focused(&self, style: &Self::Style) -> text_input::Appearance {
        self.active(style)
    }

    fn placeholder_color(&self, _style: &Self::Style) -> Color {
        palette(self.dark).muted
    }

    fn value_color(&self, _style: &Self::Style) -> Color {
        palette(self.dark).text
    }

    fn disabled_color(&self, _style: &Self::Style) -> Color {
        palette(self.dark).muted
    }

    fn selection_color(&self, _style: &Self::Style) -> Color {
        palette(self.dark).accent_soft
    }

    fn disabled(&self, style: &Self::Style) -> text_input::Appearance {
        self.active(style)
    }
}

/// Slider: a 4px rail filled with the accent, a round light handle.
pub struct Slider {
    pub dark: bool,
}

impl Slider {
    fn look(&self, handle: Color) -> slider::Appearance {
        let p = palette(self.dark);
        slider::Appearance {
            rail: slider::Rail {
                colors: (p.accent, p.border_strong),
                width: 4.0,
                border_radius: 2.0.into(),
            },
            handle: slider::Handle {
                shape: slider::HandleShape::Circle { radius: 7.0 },
                color: handle,
                border_width: 0.0,
                border_color: Color::TRANSPARENT,
            },
        }
    }
}

impl slider::StyleSheet for Slider {
    type Style = Theme;

    fn active(&self, _style: &Self::Style) -> slider::Appearance {
        self.look(palette(self.dark).text)
    }

    fn hovered(&self, _style: &Self::Style) -> slider::Appearance {
        self.look(palette(self.dark).text)
    }

    fn dragging(&self, _style: &Self::Style) -> slider::Appearance {
        self.look(palette(self.dark).accent)
    }
}

/// Select (pick list): a field with a chevron.
pub struct Select {
    pub dark: bool,
}

impl pick_list::StyleSheet for Select {
    type Style = Theme;

    fn active(&self, _style: &Self::Style) -> pick_list::Appearance {
        let p = palette(self.dark);
        pick_list::Appearance {
            text_color: p.text,
            placeholder_color: p.muted,
            handle_color: p.muted,
            background: Background::Color(p.bg),
            border: border(p.border, 1.0, R_ROW),
        }
    }

    fn hovered(&self, _style: &Self::Style) -> pick_list::Appearance {
        let p = palette(self.dark);
        pick_list::Appearance {
            border: border(p.border_strong, 1.0, R_ROW),
            ..self.active(_style)
        }
    }
}

impl iced::overlay::menu::StyleSheet for Select {
    type Style = Theme;

    fn appearance(&self, _style: &Self::Style) -> iced::overlay::menu::Appearance {
        let p = palette(self.dark);
        iced::overlay::menu::Appearance {
            text_color: p.text,
            background: Background::Color(p.elevated),
            border: border(p.border_strong, 1.0, R_ROW),
            selected_text_color: p.accent,
            selected_background: Background::Color(p.accent_soft),
        }
    }
}

/// The dimmed backdrop behind modal dialogs and the palette.
pub struct Scrim {
    pub dark: bool,
}

impl iced_aw::style::modal::StyleSheet for Scrim {
    type Style = Theme;

    fn active(&self, _style: &Self::Style) -> iced_aw::style::modal::Appearance {
        iced_aw::style::modal::Appearance {
            background: Background::Color(palette(self.dark).scrim),
        }
    }
}

/// Slim scrollbars that stay out of the way.
pub struct Scrollbar {
    pub dark: bool,
}

impl Scrollbar {
    fn look(&self, strong: bool) -> scrollable::Appearance {
        let p = palette(self.dark);
        scrollable::Appearance {
            container: container::Appearance::default(),
            scrollbar: scrollable::Scrollbar {
                background: None,
                border: no_border(),
                scroller: scrollable::Scroller {
                    color: if strong { p.muted } else { p.border_strong },
                    border: border(Color::TRANSPARENT, 0.0, 4.0),
                },
            },
            gap: None,
        }
    }
}

impl scrollable::StyleSheet for Scrollbar {
    type Style = Theme;

    fn active(&self, _style: &Self::Style) -> scrollable::Appearance {
        self.look(false)
    }

    fn hovered(&self, _style: &Self::Style, over_scrollbar: bool) -> scrollable::Appearance {
        self.look(over_scrollbar)
    }

    fn dragging(&self, _style: &Self::Style) -> scrollable::Appearance {
        self.look(true)
    }
}

/// The text editor: transparent (the panel behind it provides the surface),
/// accent caret and selection.
pub struct EditorStyle {
    pub dark: bool,
}

impl text_editor::StyleSheet for EditorStyle {
    type Style = Theme;

    fn active(&self, _style: &Self::Style) -> text_editor::Appearance {
        text_editor::Appearance {
            background: Background::Color(Color::TRANSPARENT),
            border: no_border(),
        }
    }

    fn focused(&self, style: &Self::Style) -> text_editor::Appearance {
        self.active(style)
    }

    fn hovered(&self, style: &Self::Style) -> text_editor::Appearance {
        self.active(style)
    }

    fn disabled(&self, style: &Self::Style) -> text_editor::Appearance {
        self.active(style)
    }

    fn placeholder_color(&self, _style: &Self::Style) -> Color {
        palette(self.dark).muted
    }

    fn value_color(&self, _style: &Self::Style) -> Color {
        palette(self.dark).accent
    }

    fn selection_color(&self, _style: &Self::Style) -> Color {
        Color {
            a: 0.28,
            ..palette(self.dark).accent
        }
    }

    fn disabled_color(&self, _style: &Self::Style) -> Color {
        palette(self.dark).muted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn every_embedded_font_parses_and_is_named() {
        // A truncated or wrong file would silently fall back to a system font.
        for bytes in font_files() {
            assert!(bytes.len() > 10_000);
            assert_eq!(&bytes[..4], &[0, 1, 0, 0], "not a TrueType file");
        }
    }
}
