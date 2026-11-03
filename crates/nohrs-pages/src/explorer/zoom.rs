//! The listing's zoom: one continuous value that runs from the list table up
//! through ever larger icon tiles, the way File Pilot and Explorer's
//! Ctrl+wheel do, instead of two fixed modes.

use super::types::ViewMode;

/// Zoom at or below which the listing is the list table rather than tiles.
pub const LIST_MAX: f32 = 0.1;
/// Zoom a fresh switch to the grid lands on: medium icons.
pub const DEFAULT_GRID: f32 = 0.35;
/// How far one notch of Ctrl+wheel, or one Ctrl+= / Ctrl+-, moves the zoom.
pub const STEP: f32 = 0.05;

const ICON_MIN: f32 = 48.0;
const ICON_MAX: f32 = 256.0;

/// The view a zoom value shows.
pub fn view_mode(zoom: f32) -> ViewMode {
    if zoom <= LIST_MAX {
        ViewMode::List
    } else {
        ViewMode::Grid
    }
}

/// Side of a grid tile's thumbnail, in logical pixels.
pub fn icon_size(zoom: f32) -> f32 {
    let along = ((zoom - LIST_MAX) / (1.0 - LIST_MAX)).clamp(0.0, 1.0);
    (ICON_MIN + (ICON_MAX - ICON_MIN) * along).round()
}

/// Moves `zoom` by `delta`, kept inside 0..=1.
pub fn step(zoom: f32, delta: f32) -> f32 {
    (zoom + delta).clamp(0.0, 1.0)
}

/// The name the footer shows for a zoom value.
pub fn label(zoom: f32) -> &'static str {
    if zoom <= LIST_MAX {
        return "List";
    }
    match icon_size(zoom) {
        size if size < 80.0 => "S Icons",
        size if size < 136.0 => "M Icons",
        size if size < 200.0 => "L Icons",
        _ => "XL Icons",
    }
}

/// Edge of the square thumbnail to ask for, in device pixels.
///
/// Rounded up to a few fixed sizes so dragging the zoom does not re-extract
/// every thumbnail at each pixel; the image is scaled down to the tile anyway.
pub fn thumbnail_bucket(icon_size: f32, scale_factor: f32) -> u32 {
    let wanted = (icon_size * scale_factor).ceil() as u32;
    [64, 128, 256, 512]
        .into_iter()
        .find(|&bucket| bucket >= wanted)
        .unwrap_or(512)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_below_the_threshold_tiles_above() {
        assert!(view_mode(0.0) == ViewMode::List);
        assert!(view_mode(LIST_MAX) == ViewMode::List);
        assert!(view_mode(LIST_MAX + 0.01) == ViewMode::Grid);
    }

    #[test]
    fn icon_size_spans_the_range_and_grows() {
        assert_eq!(icon_size(LIST_MAX), ICON_MIN);
        assert_eq!(icon_size(1.0), ICON_MAX);
        assert!(icon_size(0.5) > icon_size(0.3));
    }

    #[test]
    fn step_stays_in_range() {
        assert_eq!(step(0.98, STEP), 1.0);
        assert_eq!(step(0.02, -STEP), 0.0);
    }

    #[test]
    fn buckets_round_up_and_cap() {
        assert_eq!(thumbnail_bucket(48.0, 1.0), 64);
        assert_eq!(thumbnail_bucket(100.0, 1.0), 128);
        assert_eq!(thumbnail_bucket(100.0, 1.5), 256);
        assert_eq!(thumbnail_bucket(256.0, 3.0), 512);
    }
}
