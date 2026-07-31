/// Anchor geometry in viewport coordinates (as returned by
/// `getBoundingClientRect`). The overlay is `position: fixed`, so all math in
/// this module stays in viewport space — no scroll offsets.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
#[cfg_attr(not(feature = "hydrate"), allow(dead_code))]
pub(crate) struct AnchorRect {
    pub top: f64,
    pub left: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Copy, PartialEq, Debug, Default)]
#[cfg_attr(not(feature = "hydrate"), allow(dead_code))]
pub(crate) struct OverlaySize {
    pub width: f64,
    pub height: f64,
}

/// Minimum distance kept between the overlay and every viewport edge.
#[cfg_attr(not(feature = "hydrate"), allow(dead_code))]
const EDGE_MARGIN: f64 = 8.0;
/// Gap between the anchor and the overlay.
#[cfg_attr(not(feature = "hydrate"), allow(dead_code))]
const ANCHOR_GAP: f64 = 8.0;

/// Compute the `(top, left)` for a fixed-position overlay anchored to
/// `anchor`: centered above it, flipped below when there is no room above,
/// clamped to the viewport on both axes.
#[cfg_attr(not(feature = "hydrate"), allow(dead_code))]
pub(crate) fn overlay_position(
    anchor: AnchorRect,
    overlay: OverlaySize,
    viewport: OverlaySize,
) -> (f64, f64) {
    // Prefer above the anchor; flip below when the overlay would clip the top.
    let mut top = anchor.top - overlay.height - ANCHOR_GAP;
    if top < EDGE_MARGIN {
        top = anchor.top + anchor.height + ANCHOR_GAP;
    }
    // `.max(EDGE_MARGIN)` keeps the clamp range valid when the overlay is
    // larger than the viewport (f64::clamp panics when min > max).
    let max_top = (viewport.height - overlay.height - EDGE_MARGIN).max(EDGE_MARGIN);
    let top = top.clamp(EDGE_MARGIN, max_top);

    let left = anchor.left + anchor.width / 2.0 - overlay.width / 2.0;
    let max_left = (viewport.width - overlay.width - EDGE_MARGIN).max(EDGE_MARGIN);
    let left = left.clamp(EDGE_MARGIN, max_left);

    (top, left)
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIEWPORT: OverlaySize = OverlaySize {
        width: 1280.0,
        height: 800.0,
    };
    const OVERLAY: OverlaySize = OverlaySize {
        width: 200.0,
        height: 100.0,
    };

    fn anchor(top: f64, left: f64) -> AnchorRect {
        AnchorRect {
            top,
            left,
            width: 40.0,
            height: 20.0,
        }
    }

    #[test]
    fn hover_card_positions_above_and_centered_when_there_is_room() {
        let (top, left) = overlay_position(anchor(400.0, 600.0), OVERLAY, VIEWPORT);
        // 8px above the anchor, horizontally centered on it.
        assert_eq!(top, 400.0 - 100.0 - 8.0);
        assert_eq!(left, 600.0 + 20.0 - 100.0);
    }

    #[test]
    fn hover_card_flips_below_when_no_room_above() {
        let (top, _) = overlay_position(anchor(50.0, 600.0), OVERLAY, VIEWPORT);
        assert_eq!(top, 50.0 + 20.0 + 8.0);
    }

    #[test]
    fn hover_card_clamps_to_left_edge() {
        let (_, left) = overlay_position(anchor(400.0, 4.0), OVERLAY, VIEWPORT);
        assert_eq!(left, 8.0);
    }

    #[test]
    fn hover_card_clamps_to_right_edge() {
        let (_, left) = overlay_position(anchor(400.0, 1270.0), OVERLAY, VIEWPORT);
        assert_eq!(left, 1280.0 - 200.0 - 8.0);
    }

    #[test]
    fn hover_card_flipped_overlay_near_bottom_is_clamped() {
        // Anchor near the top forces a flip below; the short viewport then
        // forces the vertical clamp so the overlay never overflows the bottom.
        let viewport = OverlaySize {
            width: 1280.0,
            height: 160.0,
        };
        let (top, _) = overlay_position(anchor(40.0, 600.0), OVERLAY, viewport);
        assert_eq!(top, 160.0 - 100.0 - 8.0);
    }

    #[test]
    fn hover_card_tiny_viewport_does_not_panic_and_pins_to_margin() {
        // Overlay bigger than the viewport: both clamp ranges collapse to the
        // edge margin instead of panicking (f64::clamp panics when min > max).
        let viewport = OverlaySize {
            width: 100.0,
            height: 60.0,
        };
        let (top, left) = overlay_position(anchor(10.0, 10.0), OVERLAY, viewport);
        assert_eq!(top, 8.0);
        assert_eq!(left, 8.0);
    }
}
