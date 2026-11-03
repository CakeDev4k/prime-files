/// Modern theme tokens inspired by macOS and modern file explorers
// Consumers import this as `theme::theme` (e.g. `use nohrs_ui::theme::theme`);
// the inner module name is part of the established public path.
#[allow(clippy::module_inception)]
pub mod theme {
    use std::sync::atomic::{AtomicBool, Ordering};

    // The tokens are read during render, on every frame, by code that has no
    // `cx` to hand in some places (free helper functions building elements). A
    // process-wide flag keeps them plain calls; the appearance is chosen once at
    // startup, before the first window opens, so no frame ever reads it torn.
    static DARK: AtomicBool = AtomicBool::new(false);

    /// Selects the palette every token below reads from.
    pub fn set_dark(dark: bool) {
        DARK.store(dark, Ordering::Relaxed);
    }

    /// Whether the dark palette is the one in use.
    pub fn is_dark() -> bool {
        DARK.load(Ordering::Relaxed)
    }

    struct Palette {
        gray_50: u32,
        gray_100: u32,
        gray_200: u32,
        gray_300: u32,
        gray_400: u32,
        gray_500: u32,
        gray_600: u32,
        gray_700: u32,
        gray_800: u32,
        gray_900: u32,
        bg: u32,
        bg_secondary: u32,
        bg_hover: u32,
        fg: u32,
        fg_secondary: u32,
        muted: u32,
        border: u32,
        border_hover: u32,
        toolbar_bg: u32,
        toolbar_hover: u32,
        toolbar_text: u32,
        toolbar_active_bg: u32,
        toolbar_border: u32,
        accent: u32,
        accent_hover: u32,
        accent_subtle: u32,
        accent_fg: u32,
        danger: u32,
        warning: u32,
        preview_backdrop: u32,
    }

    const LIGHT: Palette = Palette {
        gray_50: 0xF9FAFB,
        gray_100: 0xF3F4F6,
        gray_200: 0xE5E7EB,
        gray_300: 0xD1D5DB,
        gray_400: 0x9CA3AF,
        gray_500: 0x6B7280,
        gray_600: 0x4B5563,
        gray_700: 0x374151,
        gray_800: 0x1F2937,
        gray_900: 0x111827,
        bg: 0xFFFFFF,
        bg_secondary: 0xF9FAFB,
        bg_hover: 0xF3F4F6,
        fg: 0x111827,
        fg_secondary: 0x6B7280,
        muted: 0x9CA3AF,
        border: 0xE5E7EB,
        border_hover: 0xD1D5DB,
        // Chrome surfaces (nav rail, tab bar, status bar) share one tone so the
        // window reads as a frame around the white content areas.
        toolbar_bg: 0xF3F4F6,
        toolbar_hover: 0xE5E7EB,
        toolbar_text: 0x4B5563,
        toolbar_active_bg: 0xFFFFFF,
        toolbar_border: 0xE5E7EB,
        // Accent ramp, derived from the burnt orange in the app icon. `accent`
        // is dark enough to carry `accent_fg` text at WCAG AA (4.9:1), which
        // the filled controls (search button, active scope chips) rely on.
        accent: 0xB85214,
        accent_hover: 0x93400F,
        accent_subtle: 0xFDF1E9,
        accent_fg: 0xFFFFFF,
        danger: 0xDC2626,
        warning: 0xB45309,
        preview_backdrop: 0x1F2937,
    };

    // Graphite rather than black: pure black behind near-white text is the
    // harshest contrast a screen can show and smears on OLED. The text stops
    // short of white for the same reason, and every step keeps the chrome a
    // shade darker than the content so the frame reads the same way it does in
    // the light palette.
    const DARK_PALETTE: Palette = Palette {
        // The gray ramp is inverted, since its callers mean "faint" by the low
        // end and "strong" by the high end, not "light" and "dark".
        gray_50: 0x232428,
        gray_100: 0x2A2C30,
        gray_200: 0x33353A,
        gray_300: 0x43464D,
        gray_400: 0x6E7179,
        gray_500: 0x9A9CA3,
        gray_600: 0xB4B6BC,
        gray_700: 0xC8CACF,
        gray_800: 0xDCDDE0,
        gray_900: 0xE6E7EA,
        bg: 0x1E1F22,
        bg_secondary: 0x232428,
        bg_hover: 0x2A2C30,
        fg: 0xDCDDE0,
        fg_secondary: 0x9A9CA3,
        muted: 0x6E7179,
        border: 0x2F3135,
        border_hover: 0x43464D,
        toolbar_bg: 0x18191C,
        toolbar_hover: 0x2A2C30,
        toolbar_text: 0xB4B6BC,
        toolbar_active_bg: 0x2A2C30,
        toolbar_border: 0x2A2C30,
        // The icon's orange lifted until it reads on graphite; at that
        // lightness white text on it would fall under AA, so filled controls
        // carry the background tone instead (about 6:1).
        accent: 0xE0803F,
        accent_hover: 0xEE9558,
        accent_subtle: 0x3A2B21,
        accent_fg: 0x1B1C1F,
        danger: 0xF07171,
        warning: 0xE0A33A,
        preview_backdrop: 0x141517,
    };

    fn palette() -> &'static Palette {
        if is_dark() { &DARK_PALETTE } else { &LIGHT }
    }

    macro_rules! tokens {
        ($($(#[$doc:meta])* $name:ident;)*) => {
            $(
                $(#[$doc])*
                pub fn $name() -> u32 {
                    palette().$name
                }
            )*
        };
    }

    tokens! {
        /// Faintest step of the gray ramp.
        gray_50;
        /// Faint gray, used for subtle backgrounds.
        gray_100;
        /// Gray used for default borders.
        gray_200;
        /// Gray used for borders on hover.
        gray_300;
        /// Gray used for muted text.
        gray_400;
        /// Gray used for secondary text.
        gray_500;
        /// Gray used for primary text.
        gray_600;
        /// Gray used for high-emphasis text.
        gray_700;
        /// Second-strongest step of the gray ramp.
        gray_800;
        /// Strongest step of the gray ramp.
        gray_900;
        /// Primary surface background color.
        bg;
        /// Secondary surface background color.
        bg_secondary;
        /// Background color for hovered surfaces.
        bg_hover;
        /// Primary foreground (text) color.
        fg;
        /// Secondary foreground (text) color for less prominent content.
        fg_secondary;
        /// Muted foreground color for de-emphasized content.
        muted;
        /// Default border color.
        border;
        /// Border color on hover.
        border_hover;
        /// Toolbar background color.
        toolbar_bg;
        /// Toolbar item background color on hover.
        toolbar_hover;
        /// Toolbar text color.
        toolbar_text;
        /// Background color of the active toolbar item.
        toolbar_active_bg;
        /// Toolbar border color.
        toolbar_border;
        /// Primary accent color.
        accent;
        /// Accent color on hover / pressed.
        accent_hover;
        /// Faint accent tint, for selected rows and active chip backgrounds.
        accent_subtle;
        /// Text and icon color on top of an `accent`-filled surface.
        accent_fg;
        /// Danger color, used for error messages and destructive actions.
        danger;
        /// Warning color, for an operation that did what was asked and left
        /// something unfinished. Amber rather than a lighter red: the distance
        /// from `danger` is the whole point, since the two say opposite things
        /// about whether the thing the user asked for happened.
        warning;
        /// Backdrop behind an image preview; deliberately dark so transparent
        /// and light-colored images stay legible.
        preview_backdrop;
    }

    /// Text color of the active toolbar item.
    pub fn toolbar_active_text() -> u32 {
        accent()
    }

    /// Status bar background; matches the other chrome surfaces.
    pub fn footer_bg() -> u32 {
        toolbar_bg()
    }

    /// Opacity of the scrim painted behind a modal dialog. `gpui_component`
    /// ships 5%, which is invisible against this theme's white surfaces and
    /// leaves confirmations looking like floating panels rather than modals.
    pub const MODAL_SCRIM_ALPHA: f32 = 0.40;
}
