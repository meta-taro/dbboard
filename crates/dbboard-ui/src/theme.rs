//! Central design system for dbboard's egui UI (ADR-0056, DESIGN.md).
//!
//! Before this module the app ran on stock egui styling: the default
//! `Ubuntu-Light` font, egui's built-in blue-grey palette, and a handful
//! of ad-hoc [`egui::Color32`] literals (`LIGHT_RED`, `LIGHT_GREEN`,
//! `YELLOW`) that ignored the active theme. This module replaces that with
//! a single branded palette applied through [`apply`], plus theme-aware
//! semantic colour accessors ([`danger`], [`success`], [`warning`]) that
//! the scattered call sites now read instead of hard-coding one RGB.
//!
//! ## Two themes, one call
//!
//! The app supports Light, Dark, and Auto (follow-OS) — Auto being the
//! default (ADR-0041). [`apply`] registers a customised [`egui::Visuals`]
//! for *both* concrete themes via [`egui::Context::set_visuals_of`], so
//! Auto keeps working: egui swaps between our two visuals as the OS theme
//! changes, with no per-frame reapplication. Spacing and corner-radius
//! tokens are shared across themes and set once via
//! [`egui::Context::all_styles_mut`].
//!
//! ## Palette
//!
//! Values are the tokens locked in `DESIGN.md`. Neutrals are tinted
//! toward the indigo accent (not pure grey) so the ground reads as
//! chosen rather than inherited. The accent is the brand indigo from the
//! logo (`#4F46E5` light / a brighter `#6366F1` on the dark ground so it
//! keeps its punch). Semantic colours (danger/warning/success) are a
//! separate axis from the accent and never double as it.

use egui::{Color32, CornerRadius, Stroke, Style, Theme, Visuals};

// ── Accent (brand indigo) ────────────────────────────────────────────
const ACCENT_LIGHT: Color32 = Color32::from_rgb(0x4F, 0x46, 0xE5);
const ACCENT_DARK: Color32 = Color32::from_rgb(0x63, 0x66, 0xF1);

// ── Semantic colours (separate axis from the accent) ─────────────────
const DANGER_LIGHT: Color32 = Color32::from_rgb(0xDC, 0x26, 0x26);
const DANGER_DARK: Color32 = Color32::from_rgb(0xF8, 0x71, 0x71);
const WARNING_LIGHT: Color32 = Color32::from_rgb(0xB4, 0x53, 0x09);
const WARNING_DARK: Color32 = Color32::from_rgb(0xFB, 0xBF, 0x24);
const SUCCESS_LIGHT: Color32 = Color32::from_rgb(0x05, 0x96, 0x69);
const SUCCESS_DARK: Color32 = Color32::from_rgb(0x34, 0xD3, 0x99);

// ── Neutral grounds (indigo-tinted, not pure grey) ───────────────────
const CANVAS_LIGHT: Color32 = Color32::from_rgb(0xF4, 0xF5, 0xF8);
const SURFACE_LIGHT: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);
const SURFACE_ALT_LIGHT: Color32 = Color32::from_rgb(0xF0, 0xF1, 0xF5);
const CODE_BG_LIGHT: Color32 = Color32::from_rgb(0xFA, 0xFB, 0xFC);
const BORDER_LIGHT: Color32 = Color32::from_rgb(0xE2, 0xE4, 0xEC);
const BORDER_STRONG_LIGHT: Color32 = Color32::from_rgb(0xD3, 0xD6, 0xE0);

const CANVAS_DARK: Color32 = Color32::from_rgb(0x0C, 0x0E, 0x14);
const SURFACE_DARK: Color32 = Color32::from_rgb(0x17, 0x19, 0x22);
const SURFACE_ALT_DARK: Color32 = Color32::from_rgb(0x1E, 0x21, 0x30);
const CODE_BG_DARK: Color32 = Color32::from_rgb(0x12, 0x14, 0x1C);
const BORDER_DARK: Color32 = Color32::from_rgb(0x28, 0x2C, 0x39);
const BORDER_STRONG_DARK: Color32 = Color32::from_rgb(0x33, 0x38, 0x49);

// ── Spacing & radius tokens (shared across themes) ───────────────────
const WIDGET_RADIUS: u8 = 6;
const WINDOW_RADIUS: u8 = 8;

/// Text/glyph colour for filled accent surfaces (the primary button, a
/// selected segment). Near-white in both themes: the accent indigo is
/// saturated enough that white clears contrast on either shade, so one
/// token serves both rather than a per-theme pair.
const ON_ACCENT: Color32 = Color32::from_rgb(0xFA, 0xFB, 0xFF);

/// Brand accent for the active theme.
///
/// These accessors are the canonical palette source: [`brand`] sets the
/// matching egui `Visuals` fields (`hyperlink_color`, `error_fg_color`,
/// `warn_fg_color`) *from* them, so call sites read the accessor rather
/// than the derived `Visuals` field. That keeps every colour site pointed
/// at one definition and works even where no `apply`-ed `Visuals` is in
/// scope — the caller only needs `ui.visuals().dark_mode`.
#[must_use]
pub fn accent(dark_mode: bool) -> Color32 {
    if dark_mode {
        ACCENT_DARK
    } else {
        ACCENT_LIGHT
    }
}

/// Destructive / error colour for the active theme (replaces ad-hoc
/// `Color32::LIGHT_RED`). Matches `visuals.error_fg_color`.
#[must_use]
pub fn danger(dark_mode: bool) -> Color32 {
    if dark_mode {
        DANGER_DARK
    } else {
        DANGER_LIGHT
    }
}

/// Caution colour for the active theme (replaces ad-hoc
/// `Color32::YELLOW`). Matches `visuals.warn_fg_color`.
#[must_use]
pub fn warning(dark_mode: bool) -> Color32 {
    if dark_mode {
        WARNING_DARK
    } else {
        WARNING_LIGHT
    }
}

/// Healthy / OK colour for the active theme (replaces ad-hoc
/// `Color32::LIGHT_GREEN`).
#[must_use]
pub fn success(dark_mode: bool) -> Color32 {
    if dark_mode {
        SUCCESS_DARK
    } else {
        SUCCESS_LIGHT
    }
}

/// A filled, accent-coloured primary button — the one call-to-action per
/// view (Run). egui ships no "primary" button style, so this composes one:
/// the [`accent`] as the fill with a near-white bold label on top, matching
/// the mock's Run affordance. `dark_mode` selects the accent shade; take it
/// from `ui.visuals().dark_mode` at the call site.
pub fn primary_button(dark_mode: bool, text: impl Into<String>) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(text.into()).color(ON_ACCENT).strong())
        .fill(accent(dark_mode))
}

/// A compact rounded "chip" — a faint-filled, bordered badge for a short
/// status label (the header's active-connection pill, a sidebar count).
/// When `accent_dot` is `Some`, a small coloured dot prefixes the text
/// (e.g. the brand accent marking the live connection); `None` draws a
/// plain chip. Corner radius and colours come from the active theme's
/// tokens so the chip matches the surrounding chrome in both themes.
pub fn pill(ui: &mut egui::Ui, text: &str, accent_dot: Option<Color32>) {
    egui::Frame::new()
        .fill(ui.visuals().faint_bg_color)
        .stroke(ui.visuals().widgets.noninteractive.bg_stroke)
        .corner_radius(CornerRadius::same(WIDGET_RADIUS))
        .inner_margin(egui::Margin::symmetric(8, 2))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                if let Some(dot) = accent_dot {
                    // U+25CF BLACK CIRCLE: a glyph dot avoids hand-painting a
                    // circle and is covered by the bundled fonts.
                    ui.label(egui::RichText::new("\u{25CF}").small().color(dot));
                }
                ui.label(egui::RichText::new(text).small());
            });
        });
}

/// Apply the dbboard design system to `ctx`.
///
/// Registers a branded [`Visuals`] for both Light and Dark so Auto
/// (follow-OS) swaps between them for free, then sets the shared spacing
/// and corner-radius tokens on every style. Call once at startup, after
/// fonts are installed and before the first paint.
pub fn apply(ctx: &egui::Context) {
    ctx.set_visuals_of(Theme::Dark, dark_visuals());
    ctx.set_visuals_of(Theme::Light, light_visuals());
    // Spacing/radius are theme-independent; `all_styles_mut` touches only
    // the fields we set, leaving the visuals registered above intact.
    ctx.all_styles_mut(apply_spacing);
}

/// The branded dark-theme visuals.
#[must_use]
pub fn dark_visuals() -> Visuals {
    brand(Visuals::dark(), true)
}

/// The branded light-theme visuals.
#[must_use]
pub fn light_visuals() -> Visuals {
    brand(Visuals::light(), false)
}

/// Overlay the dbboard palette onto egui's stock dark/light visuals.
/// Starts from the stock base so every field we do not touch keeps a
/// sensible default; we override only the ones that carry the brand.
fn brand(mut v: Visuals, dark: bool) -> Visuals {
    let (canvas, surface, surface_alt, code_bg, border, border_strong) = if dark {
        (
            CANVAS_DARK,
            SURFACE_DARK,
            SURFACE_ALT_DARK,
            CODE_BG_DARK,
            BORDER_DARK,
            BORDER_STRONG_DARK,
        )
    } else {
        (
            CANVAS_LIGHT,
            SURFACE_LIGHT,
            SURFACE_ALT_LIGHT,
            CODE_BG_LIGHT,
            BORDER_LIGHT,
            BORDER_STRONG_LIGHT,
        )
    };
    let accent = accent(dark);

    // Accent drives links and selection. The selection fill is the accent
    // at low alpha so selected text/rows read as a translucent tint that
    // keeps the text underneath legible in both themes; the outline is the
    // opaque accent. (The staged-edit tint in lib.rs keys off the accent
    // directly, not this fill, because a premultiplied translucent colour
    // reads back dimmed.)
    v.hyperlink_color = accent;
    v.selection.bg_fill = Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 60);
    v.selection.stroke = Stroke::new(1.0, accent);

    // Semantic colours as a separate axis; egui's own warn/error text
    // picks these up, matching the `danger`/`warning` accessors.
    v.error_fg_color = danger(dark);
    v.warn_fg_color = warning(dark);

    // Grounds. `panel_fill` is the app canvas, `window_fill` the floating
    // dialogs, `faint_bg_color` the zebra stripe / hovered row,
    // `extreme_bg_color` the text-edit field, `code_bg_color` inline code.
    v.panel_fill = canvas;
    v.window_fill = surface;
    v.window_stroke = Stroke::new(1.0, border);
    v.faint_bg_color = surface_alt;
    v.extreme_bg_color = surface_alt;
    v.code_bg_color = code_bg;

    // Rounded, editor-like chrome.
    v.window_corner_radius = CornerRadius::same(WINDOW_RADIUS);
    v.menu_corner_radius = CornerRadius::same(WINDOW_RADIUS);
    let radius = CornerRadius::same(WIDGET_RADIUS);
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = radius;
    }
    // A hair firmer separators than stock so panels read as distinct
    // without a heavy border.
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, border);
    v.widgets.hovered.bg_stroke = Stroke::new(1.0, border_strong);

    v
}

/// Shared spacing/radius tokens (see `DESIGN.md`). Applied to every
/// style regardless of theme.
fn apply_spacing(style: &mut Style) {
    // Base unit 4px: comfortable item spacing and roomier buttons than
    // egui's tight defaults, matching the approved mock.
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 6.0);
    style.spacing.menu_margin = egui::Margin::same(6);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accent_is_the_brand_indigo_per_theme() {
        assert_eq!(accent(false), Color32::from_rgb(0x4F, 0x46, 0xE5));
        assert_eq!(accent(true), Color32::from_rgb(0x63, 0x66, 0xF1));
    }

    #[test]
    fn semantic_colours_switch_with_the_theme() {
        // Each semantic colour has a distinct value per theme, and none
        // of them collide with the accent (separate axis).
        assert_ne!(danger(true), danger(false));
        assert_ne!(warning(true), warning(false));
        assert_ne!(success(true), success(false));
        for dark in [true, false] {
            assert_ne!(danger(dark), accent(dark));
            assert_ne!(warning(dark), accent(dark));
            assert_ne!(success(dark), accent(dark));
        }
    }

    #[test]
    fn dark_visuals_are_dark_and_branded() {
        let v = dark_visuals();
        assert!(v.dark_mode);
        assert_eq!(v.hyperlink_color, accent(true));
        assert_eq!(v.error_fg_color, danger(true));
        assert_eq!(v.warn_fg_color, warning(true));
        assert_eq!(v.panel_fill, CANVAS_DARK);
        // Selection outline is the opaque accent; the fill is a translucent
        // accent tint so selected text stays legible.
        assert_eq!(v.selection.stroke.color, accent(true));
        assert!(v.selection.bg_fill.a() < 255);
    }

    #[test]
    fn light_visuals_are_light_and_branded() {
        let v = light_visuals();
        assert!(!v.dark_mode);
        assert_eq!(v.hyperlink_color, accent(false));
        assert_eq!(v.error_fg_color, danger(false));
        assert_eq!(v.panel_fill, CANVAS_LIGHT);
    }

    #[test]
    fn widget_corner_radius_is_the_token() {
        let v = dark_visuals();
        let expect = CornerRadius::same(WIDGET_RADIUS);
        // Every interactive state carries the token, not just a sample —
        // a regression in one state would otherwise slip through.
        for w in [
            &v.widgets.noninteractive,
            &v.widgets.inactive,
            &v.widgets.hovered,
            &v.widgets.active,
            &v.widgets.open,
        ] {
            assert_eq!(w.corner_radius, expect);
        }
        assert_eq!(v.window_corner_radius, CornerRadius::same(WINDOW_RADIUS));
    }

    #[test]
    fn on_accent_is_opaque_and_distinct_from_the_accent() {
        // The primary button paints ON_ACCENT text over an `accent` fill, so
        // the two must differ (contrast) and the text must be fully opaque
        // (a translucent label would read dimmed over the fill).
        assert_eq!(ON_ACCENT.a(), 255);
        for dark in [true, false] {
            assert_ne!(ON_ACCENT, accent(dark));
        }
    }

    #[test]
    fn spacing_tokens_are_applied() {
        let mut style = Style::default();
        apply_spacing(&mut style);
        assert_eq!(style.spacing.item_spacing, egui::vec2(8.0, 6.0));
        assert_eq!(style.spacing.button_padding, egui::vec2(10.0, 6.0));
    }
}
