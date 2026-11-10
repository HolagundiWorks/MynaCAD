//! Design tokens. Colours were measured from the reference look (dark slate chrome) and are
//! AAD's own values; every widget reads them from here.
//!
//! Two palettes, switched by one toggle: [`Tokens::DARK`] (black drawing area) and
//! [`Tokens::LIGHT`] (white drawing area). The theme changes the chrome (title and tool bar,
//! menus, palettes, tabs, command line, status bar, dialogs) and the drawing area's background,
//! grid and cursor. There is no other colour customisation. [`Tokens::get`] returns the palette
//! of the theme in use, which [`CadApp::logic`](crate::CadApp::logic) resolves once per frame
//! from the user's [`ThemePref`].

use std::cell::Cell;

use egui::{Color32, FontFamily, FontId, Theme, Visuals};
use serde::{Deserialize, Serialize};

/// The interface theme the user chose.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemePref {
    Light,
    /// The default: AAD's original look.
    #[default]
    Dark,
}

impl ThemePref {
    pub const ALL: [ThemePref; 2] = [ThemePref::Light, ThemePref::Dark];

    pub fn as_str(self) -> &'static str {
        match self {
            ThemePref::Light => "light",
            ThemePref::Dark => "dark",
        }
    }

    pub fn parse(s: &str) -> Option<ThemePref> {
        ThemePref::ALL.into_iter().find(|t| t.as_str().eq_ignore_ascii_case(s.trim()))
    }

    /// The theme to show.
    pub fn resolve(self) -> Theme {
        match self {
            ThemePref::Light => Theme::Light,
            ThemePref::Dark => Theme::Dark,
        }
    }

    /// The other theme.
    pub fn toggled(self) -> ThemePref {
        match self {
            ThemePref::Light => ThemePref::Dark,
            ThemePref::Dark => ThemePref::Light,
        }
    }
}

thread_local! {
    /// The theme in use on the UI thread (set by [`set_active`] each frame).
    static ACTIVE: Cell<Theme> = const { Cell::new(Theme::Dark) };
}

/// The theme [`Tokens::get`] returns colours for.
pub fn active() -> Theme {
    ACTIVE.with(Cell::get)
}

pub fn set_active(theme: Theme) {
    ACTIVE.with(|a| a.set(theme));
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tokens {
    pub chrome: Color32,
    pub chrome_dark: Color32,
    pub panel: Color32,
    pub tab_active: Color32,
    pub control: Color32,
    pub control_hover: Color32,
    pub border: Color32,
    pub canvas: Color32,
    pub axis_x: Color32,
    pub axis_y: Color32,
    pub text: Color32,
    pub text_dim: Color32,
    pub text_faint: Color32,
    pub icon: Color32,
    pub icon_accent: Color32,
    pub icon_point: Color32,
    pub accent: Color32,
    /// Text and marks drawn on an `accent` fill.
    pub on_accent: Color32,
    pub selection: Color32,
    pub hover: Color32,
    pub grip: Color32,
    pub grip_hot: Color32,
    pub snap: Color32,
    pub window_fill: Color32,
    pub crossing_fill: Color32,
    pub window_stroke: Color32,
    pub crossing_stroke: Color32,
    pub cmd_bg: Color32,
    pub toggle_on: Color32,
    /// Text fields and other inset (extreme) backgrounds.
    pub field: Color32,
    /// Selected text and selected list rows.
    pub select_bg: Color32,
    /// Lines and labels drawn straight onto the drawing area (UCS icon, viewport label, rubber
    /// bands): they follow the model-space background, not the interface theme.
    pub canvas_ink: Color32,
    /// The command-line history lines behind the input bar.
    pub cmd_history: Color32,
    pub cmd_border: Color32,
    /// Command-line keywords (`[Undo/Close]`) and their hover background.
    pub cmd_keyword: Color32,
    pub cmd_keyword_hover: Color32,
    /// The command-line AutoComplete list: background and the highlighted first row.
    pub list_bg: Color32,
    pub list_row: Color32,
    /// Transient warnings (status bar messages).
    pub warn: Color32,
    /// The drawing-area crosshair cursor and its pick box.
    pub crosshair: Color32,
}

impl Tokens {
    pub const DARK: Tokens = Tokens {
        chrome: Color32::from_rgb(0x00, 0x00, 0x00),
        chrome_dark: Color32::from_rgb(0x00, 0x00, 0x00),
        panel: Color32::from_rgb(0x00, 0x00, 0x00),
        tab_active: Color32::from_rgb(0x26, 0x26, 0x26),
        control: Color32::from_rgb(0x14, 0x14, 0x14),
        control_hover: Color32::from_rgb(0x26, 0x26, 0x26),
        border: Color32::from_rgb(0x33, 0x33, 0x33),
        canvas: Color32::from_rgb(0x00, 0x00, 0x00),
        axis_x: Color32::from_rgb(0x6a, 0x2a, 0x2c),
        axis_y: Color32::from_rgb(0x2a, 0x58, 0x32),
        text: Color32::from_rgb(0xff, 0xff, 0xff),
        text_dim: Color32::from_rgb(0xb8, 0xb8, 0xb8),
        text_faint: Color32::from_rgb(0x8c, 0x8c, 0x8c),
        icon: Color32::from_rgb(0xff, 0xff, 0xff),
        icon_accent: Color32::from_rgb(0xff, 0xff, 0xff),
        icon_point: Color32::from_rgb(0xf0, 0x7a, 0x3a),
        accent: Color32::from_rgb(0xff, 0xff, 0xff),
        on_accent: Color32::from_rgb(0x00, 0x00, 0x00),
        selection: Color32::from_rgb(0x4a, 0x8f, 0xff),
        hover: Color32::from_rgb(0x9c, 0xc6, 0xff),
        grip: Color32::from_rgb(0x2f, 0x6b, 0xff),
        grip_hot: Color32::from_rgb(0xff, 0x3b, 0x3b),
        snap: Color32::from_rgb(0x2f, 0xd0, 0x62),
        window_fill: Color32::from_rgba_premultiplied(0x18, 0x30, 0x60, 0x50),
        crossing_fill: Color32::from_rgba_premultiplied(0x18, 0x48, 0x18, 0x50),
        window_stroke: Color32::from_rgb(0x6f, 0xa8, 0xff),
        crossing_stroke: Color32::from_rgb(0x7f, 0xe0, 0x7f),
        cmd_bg: Color32::from_rgba_premultiplied(0x00, 0x00, 0x00, 0xf2),
        toggle_on: Color32::from_rgb(0xf2, 0x84, 0x1c),
        field: Color32::from_rgb(0x00, 0x00, 0x00),
        select_bg: Color32::from_rgb(0xa8, 0x5a, 0x10),
        canvas_ink: Color32::from_rgb(0xb8, 0xb8, 0xb8),
        cmd_history: Color32::from_rgba_unmultiplied_const(0x00, 0x00, 0x00, 190),
        cmd_border: Color32::from_rgb(0x4d, 0x4d, 0x4d),
        cmd_keyword: Color32::from_rgb(0xff, 0xa8, 0x55),
        cmd_keyword_hover: Color32::from_rgb(0x6a, 0x3c, 0x0c),
        list_bg: Color32::from_rgb(0x00, 0x00, 0x00),
        list_row: Color32::from_rgb(0x1f, 0x1f, 0x1f),
        warn: Color32::from_rgb(0xff, 0xc8, 0x6e),
        crosshair: Color32::from_rgb(0xff, 0xff, 0xff),
    };

    /// Pure white chrome with black ink and a white drawing area. The grips and snap
    /// markers are the same as [`Tokens::DARK`]'s.
    pub const LIGHT: Tokens = Tokens {
        chrome: Color32::from_rgb(0xff, 0xff, 0xff),
        chrome_dark: Color32::from_rgb(0xd9, 0xd9, 0xd9),
        panel: Color32::from_rgb(0xff, 0xff, 0xff),
        tab_active: Color32::from_rgb(0xff, 0xff, 0xff),
        control: Color32::from_rgb(0xf0, 0xf0, 0xf0),
        control_hover: Color32::from_rgb(0xe0, 0xe0, 0xe0),
        border: Color32::from_rgb(0xd9, 0xd9, 0xd9),
        text: Color32::from_rgb(0x00, 0x00, 0x00),
        text_dim: Color32::from_rgb(0x4d, 0x4d, 0x4d),
        text_faint: Color32::from_rgb(0x76, 0x76, 0x76),
        icon: Color32::from_rgb(0x1a, 0x1a, 0x1a),
        icon_accent: Color32::from_rgb(0x00, 0x00, 0x00),
        icon_point: Color32::from_rgb(0xd3, 0x5a, 0x1c),
        accent: Color32::from_rgb(0x00, 0x00, 0x00),
        on_accent: Color32::from_rgb(0xff, 0xff, 0xff),
        cmd_bg: Color32::from_rgba_unmultiplied_const(0xff, 0xff, 0xff, 0xf2),
        toggle_on: Color32::from_rgb(0x00, 0x00, 0x00),
        field: Color32::from_rgb(0xff, 0xff, 0xff),
        select_bg: Color32::from_rgb(0xe0, 0xe0, 0xe0),
        cmd_history: Color32::from_rgba_unmultiplied_const(0xff, 0xff, 0xff, 0xe0),
        cmd_border: Color32::from_rgb(0xd9, 0xd9, 0xd9),
        cmd_keyword: Color32::from_rgb(0x00, 0x00, 0x00),
        cmd_keyword_hover: Color32::from_rgb(0xf0, 0xf0, 0xf0),
        list_bg: Color32::from_rgb(0xff, 0xff, 0xff),
        list_row: Color32::from_rgb(0xf0, 0xf0, 0xf0),
        warn: Color32::from_rgb(0x8a, 0x4b, 0x00),
        canvas: Color32::from_rgb(0xff, 0xff, 0xff),
        axis_x: Color32::from_rgb(0xe0, 0x9a, 0x9a),
        axis_y: Color32::from_rgb(0x8f, 0xc9, 0x9a),
        canvas_ink: Color32::from_rgb(0x4d, 0x4d, 0x4d),
        crosshair: Color32::from_rgb(0x00, 0x00, 0x00),
        hover: Color32::from_rgb(0x1b, 0x6f, 0xd6),
        selection: Color32::from_rgb(0x1b, 0x6f, 0xd6),
        window_fill: Color32::from_rgba_premultiplied(0x10, 0x20, 0x40, 0x30),
        crossing_fill: Color32::from_rgba_premultiplied(0x10, 0x30, 0x10, 0x30),
        window_stroke: Color32::from_rgb(0x1b, 0x6f, 0xd6),
        crossing_stroke: Color32::from_rgb(0x1e, 0x8a, 0x34),
        ..Tokens::DARK
    };

    /// The palette of `theme`.
    pub fn of(theme: Theme) -> Tokens {
        match theme {
            Theme::Dark => Tokens::DARK,
            Theme::Light => Tokens::LIGHT,
        }
    }

    /// The palette of the theme in use.
    pub fn get() -> Tokens {
        Tokens::of(active())
    }
}

// Whether the named font families (condensed, heading) exist in the font set the context is
// using. They are installed with the fonts and egui applies a new font set at the start of the
// next frame, so styles may use them only from the frame after [`install_fonts`].
thread_local! {
    static NAMED_READY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Set for the interface thread only (the thread that owns the egui context).
pub fn set_named_ready(ready: bool) {
    NAMED_READY.with(|c| c.set(ready));
}

pub fn named_ready() -> bool {
    NAMED_READY.with(std::cell::Cell::get)
}

/// Compact text (toolbar labels, metadata): the preset's Condensed face once it is available.
pub fn small() -> FontId {
    let family = if named_ready() { FontFamily::Name(CONDENSED_FAMILY.into()) } else { FontFamily::Proportional };
    FontId::new(11.5, family)
}
/// Headings: the preset's heading weight (Semibold where installed) once it is available.
pub fn heading() -> FontId {
    let family = if named_ready() { FontFamily::Name(HEADING_FAMILY.into()) } else { FontFamily::Proportional };
    FontId::new(15.0, family)
}
pub fn body() -> FontId {
    FontId::new(13.0, FontFamily::Proportional)
}
pub fn mono() -> FontId {
    FontId::new(12.0, FontFamily::Monospace)
}

/// Build the interface fonts: the selected typography preset first (Sans for text, Mono for the
/// command bar and values, Condensed under its own family name), then a system font so characters
/// the preset lacks still draw. A preset whose fonts are not installed leaves egui's and the system
/// font as they were.
pub fn install_fonts(ctx: &egui::Context, preset: &str) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use aad_fonts::registry::{self, Face};
        let candidates: &[&str] = if cfg!(target_os = "macos") {
            &["/System/Library/Fonts/SFNS.ttf", "/System/Library/Fonts/Helvetica.ttc", "/Library/Fonts/Arial.ttf"]
        } else if cfg!(windows) {
            &[r"C:\Windows\Fonts\segoeui.ttf", r"C:\Windows\Fonts\arial.ttf"]
        } else {
            &[
                "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
                "/usr/share/fonts/TTF/DejaVuSans.ttf",
                "/usr/share/fonts/noto/NotoSans-Regular.ttf",
                "/usr/share/fonts/truetype/noto/NotoSans-Regular.ttf",
            ]
        };
        let mut fonts = egui::FontDefinitions::default();
        for path in candidates {
            if let Ok(bytes) = std::fs::read(path) {
                fonts.font_data.insert("system-ui".into(), std::sync::Arc::new(egui::FontData::from_owned(bytes)));
                if let Some(f) = fonts.families.get_mut(&FontFamily::Proportional) {
                    f.insert(0, "system-ui".into());
                }
                break;
            }
        }
        // The preset's own fonts, where they are installed.
        let p = registry::preset(preset).unwrap_or(&registry::PRESETS[0]);
        let mut load = |face: Face, key: &str| -> bool {
            let Some(found) = aad_fonts::ttf::find(p.family(face)) else { return false };
            let mut data = egui::FontData::from_owned(found.bytes.as_ref().clone());
            data.index = found.index;
            fonts.font_data.insert(key.into(), std::sync::Arc::new(data));
            true
        };
        if load(Face::Sans, "aad-sans")
            && let Some(f) = fonts.families.get_mut(&FontFamily::Proportional)
        {
            f.insert(0, "aad-sans".into());
        }
        if load(Face::Mono, "aad-mono")
            && let Some(f) = fonts.families.get_mut(&FontFamily::Monospace)
        {
            f.insert(0, "aad-mono".into());
        }
        // The named families always exist (the interface styles use them): the preset's face when it
        // is installed, else the Proportional chain.
        let proportional = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
        let mut condensed = Vec::new();
        if load(Face::Condensed, "aad-condensed") {
            condensed.push("aad-condensed".to_string());
        }
        condensed.extend(proportional.iter().cloned());
        fonts.families.insert(FontFamily::Name(CONDENSED_FAMILY.into()), condensed);
        let (_, weight) = registry::Role::Heading.default_face();
        let mut heading = Vec::new();
        if let Some(found) = registry::find_weighted(p.family(Face::Sans), weight) {
            let mut data = egui::FontData::from_owned(found.bytes.as_ref().clone());
            data.index = found.index;
            fonts.font_data.insert("aad-heading".into(), std::sync::Arc::new(data));
            heading.push("aad-heading".to_string());
        }
        heading.extend(proportional);
        fonts.families.insert(FontFamily::Name(HEADING_FAMILY.into()), heading);
        ctx.set_fonts(fonts);
        true
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (ctx, preset);
        false
    }
}

/// The egui family name of the preset's heading weight (Semibold when installed).
pub const HEADING_FAMILY: &str = "aad-heading";

/// The egui family name of the preset's Condensed face (toolbar labels, compact metadata). Falls
/// back to the Proportional chain when the face is not installed.
pub const CONDENSED_FAMILY: &str = "aad-condensed";

/// egui's visuals for `theme`, from its tokens.
fn visuals(theme: Theme) -> Visuals {
    let t = Tokens::of(theme);
    let mut v = match theme {
        Theme::Dark => Visuals::dark(),
        Theme::Light => Visuals::light(),
    };
    v.panel_fill = t.panel;
    v.window_fill = t.chrome;
    v.extreme_bg_color = t.field;
    v.faint_bg_color = t.chrome_dark;
    v.override_text_color = Some(t.text);
    v.widgets.noninteractive.bg_fill = t.panel;
    v.widgets.noninteractive.fg_stroke.color = t.text;
    v.widgets.noninteractive.bg_stroke.color = t.border;
    v.widgets.inactive.bg_fill = t.control;
    v.widgets.inactive.weak_bg_fill = t.control;
    v.widgets.inactive.fg_stroke.color = t.text;
    v.widgets.hovered.bg_fill = t.control_hover;
    v.widgets.hovered.weak_bg_fill = t.control_hover;
    v.widgets.active.bg_fill = t.accent;
    v.widgets.active.weak_bg_fill = t.accent;
    v.widgets.active.fg_stroke.color = t.on_accent;
    v.selection.bg_fill = t.select_bg;
    v.window_stroke = egui::Stroke::new(1.0, t.border);
    let shadow = Color32::from_black_alpha(if theme == Theme::Dark { 90 } else { 40 });
    v.popup_shadow = egui::epaint::Shadow { offset: [0, 4], blur: 12, spread: 0, color: shadow };
    // Suite scale canon: control radius 12, overlay/menu radius 14.
    v.window_corner_radius = egui::CornerRadius::same(12);
    v.menu_corner_radius = egui::CornerRadius::same(14);
    v
}

/// Install the styles of both themes and show `theme` (egui's own widgets, popups and menus
/// follow it; our own painting follows [`Tokens::get`]).
pub fn apply(ctx: &egui::Context, theme: Theme) {
    set_active(theme);
    ctx.set_visuals_of(Theme::Dark, visuals(Theme::Dark));
    ctx.set_visuals_of(Theme::Light, visuals(Theme::Light));
    ctx.set_theme(egui::ThemePreference::from(theme));
    ctx.all_styles_mut(|s| {
        // Match O.Ui SuiteStyles: 8/12 stack · 16×6 control pad · 36 hit.
        s.spacing.item_spacing = egui::vec2(8.0, 8.0);
        s.spacing.button_padding = egui::vec2(16.0, 6.0);
        s.spacing.interact_size.y = 36.0;
        s.text_styles.insert(egui::TextStyle::Body, body());
        s.text_styles.insert(egui::TextStyle::Button, body());
        s.text_styles.insert(egui::TextStyle::Small, small());
        s.text_styles.insert(egui::TextStyle::Heading, heading());
        s.text_styles.insert(egui::TextStyle::Monospace, mono());
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lum(c: Color32) -> f64 {
        let f = |v: u8| {
            let v = f64::from(v) / 255.0;
            if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
        };
        0.2126 * f(c.r()) + 0.7152 * f(c.g()) + 0.0722 * f(c.b())
    }

    fn contrast(a: Color32, b: Color32) -> f64 {
        let (x, y) = (lum(a), lum(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn the_black_accent_is_legible_in_both_palettes() {
        for theme in [Theme::Dark, Theme::Light] {
            let t = Tokens::of(theme);
            assert!(contrast(t.on_accent, t.accent) >= 4.5, "{theme:?}: text on the accent fill");
            assert!(contrast(t.accent, t.panel) >= 3.0, "{theme:?}: accent outlines and marks on the panel");
            assert!(contrast(t.icon_accent, t.panel) >= 3.0, "{theme:?}: accented icon strokes");
            assert!(contrast(t.text, t.panel) >= 4.5, "{theme:?}: body text");
        }
    }

    #[test]
    fn the_accent_is_not_orange_any_more() {
        for theme in [Theme::Dark, Theme::Light] {
            let a = Tokens::of(theme).accent;
            // Near-black ink or near-white: a small spread, and never warm (red above blue).
            let spread = a.r().max(a.g()).max(a.b()) - a.r().min(a.g()).min(a.b());
            assert!(spread <= 32 && u16::from(a.r()) <= u16::from(a.b()) + 8, "{theme:?}: accent {a:?}");
        }
    }
}
