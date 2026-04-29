use super::super::*;

// -- pane_local_coords / pane_local_coords_clamped ---------------

#[test]
fn pane_local_coords_rejects_border_clicks() {
    // A 10×5 pane at origin (2, 3): border at col 2 / col 11 /
    // row 3 / row 7. A click on any border cell must decline to
    // forward so the caller can fall through to the border-drag
    // handler instead.
    let rect = Rect::new(2, 3, 10, 5);
    assert!(pane_local_coords(rect, 2, 5).is_none(), "left border");
    assert!(pane_local_coords(rect, 11, 5).is_none(), "right border");
    assert!(pane_local_coords(rect, 5, 3).is_none(), "top border");
    assert!(pane_local_coords(rect, 5, 7).is_none(), "bottom border");
}

#[test]
fn pane_local_coords_translates_to_content_0_origin() {
    // Pane outer at (2, 3), content starts at (3, 4). A click at
    // screen (3, 4) must land on content (0, 0); (10, 6) maps
    // to (7, 2).
    let rect = Rect::new(2, 3, 10, 5);
    assert_eq!(pane_local_coords(rect, 3, 4), Some((0, 0)));
    assert_eq!(pane_local_coords(rect, 10, 6), Some((7, 2)));
}

#[test]
fn pane_local_coords_clamped_stays_inside_content() {
    // Clamp is used on Drag/Up where the cursor may wander off-
    // pane. Ensure clamp never produces an out-of-bounds cell.
    let rect = Rect::new(2, 3, 10, 5);
    // Cursor well to the right of the pane — should pin to the
    // last content column (width - 2 = 8 inner cells, 0..=7).
    assert_eq!(pane_local_coords_clamped(rect, 50, 50), (7, 2));
    // Cursor above/left of the pane — should pin to (0, 0).
    assert_eq!(pane_local_coords_clamped(rect, 0, 0), (0, 0));
    // Cursor inside — untouched.
    assert_eq!(pane_local_coords_clamped(rect, 5, 5), (2, 1));
}

#[test]
fn pane_local_coords_rejects_rects_too_small_for_content() {
    // A 2×2 or narrower rect has no interior after stripping the
    // 1-cell border. Codex review flagged that the pre-fix version
    // underflowed with `rect.width == 1`; the guard keeps such a
    // press from ever reaching the forward path.
    for (w, h) in [(0, 5), (1, 5), (2, 5), (5, 0), (5, 1), (5, 2)] {
        let rect = Rect::new(2, 3, w, h);
        assert!(
            pane_local_coords(rect, 3, 4).is_none(),
            "{}×{} rect must be rejected before the arithmetic fires",
            w,
            h
        );
    }
}

#[test]
fn pane_local_coords_survives_extreme_u16_origins() {
    // `rect.x = u16::MAX - 5` means `rect.x + rect.width` would
    // overflow in unchecked arithmetic. Saturating math keeps this
    // from panicking in debug builds.
    let rect = Rect::new(u16::MAX - 5, 0, 10, 5);
    // The call must return, not panic. Result content is
    // secondary — whatever it yields, it yields safely.
    let _ = pane_local_coords(rect, u16::MAX - 3, 2);
    let _ = pane_local_coords_clamped(rect, u16::MAX, u16::MAX);
}

// -- mouse_forward_disabled env gate -----------------------------

#[test]
fn mouse_forward_disabled_reads_env_var() {
    // Serialize against the global env so parallel tests don't
    // see each other's values. We only toggle within this test.
    use std::sync::Mutex;
    static LOCK: Mutex<()> = Mutex::new(());
    let _guard = LOCK.lock().unwrap();

    std::env::remove_var("RENGA_DISABLE_MOUSE_FORWARD");
    assert!(!mouse_forward_disabled(), "unset → false");

    std::env::set_var("RENGA_DISABLE_MOUSE_FORWARD", "1");
    assert!(mouse_forward_disabled(), "\"1\" → true");

    std::env::set_var("RENGA_DISABLE_MOUSE_FORWARD", "0");
    assert!(
        !mouse_forward_disabled(),
        "\"0\" must be treated as opt-in-off, matching the wheel-handler convention"
    );

    std::env::set_var("RENGA_DISABLE_MOUSE_FORWARD", "");
    assert!(!mouse_forward_disabled(), "empty string → false");

    std::env::set_var("RENGA_DISABLE_MOUSE_FORWARD", "yes");
    assert!(
        mouse_forward_disabled(),
        "any non-empty non-\"0\" value → true (permissive)"
    );

    std::env::remove_var("RENGA_DISABLE_MOUSE_FORWARD");
}
