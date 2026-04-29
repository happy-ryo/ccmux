use super::super::*;

#[test]
fn test_layout_single_pane() {
    let layout = LayoutNode::Leaf { pane_id: 1 };
    assert_eq!(layout.pane_count(), 1);
    assert_eq!(layout.collect_pane_ids(), vec![1]);
}

#[test]
fn test_layout_split_vertical() {
    let mut layout = LayoutNode::Leaf { pane_id: 1 };
    layout.split_pane(1, 2, SplitDirection::Vertical);
    assert_eq!(layout.pane_count(), 2);
    assert_eq!(layout.collect_pane_ids(), vec![1, 2]);
}

#[test]
fn test_layout_split_horizontal() {
    let mut layout = LayoutNode::Leaf { pane_id: 1 };
    layout.split_pane(1, 2, SplitDirection::Horizontal);
    assert_eq!(layout.pane_count(), 2);
}

#[test]
fn test_layout_nested_split() {
    let mut layout = LayoutNode::Leaf { pane_id: 1 };
    layout.split_pane(1, 2, SplitDirection::Vertical);
    layout.split_pane(1, 3, SplitDirection::Horizontal);
    assert_eq!(layout.pane_count(), 3);
    assert_eq!(layout.collect_pane_ids(), vec![1, 3, 2]);
}

#[test]
fn test_layout_remove_pane() {
    let mut layout = LayoutNode::Leaf { pane_id: 1 };
    layout.split_pane(1, 2, SplitDirection::Vertical);
    layout.remove_pane(2);
    assert_eq!(layout.pane_count(), 1);
    assert_eq!(layout.collect_pane_ids(), vec![1]);
}

#[test]
fn test_layout_remove_first_pane() {
    let mut layout = LayoutNode::Leaf { pane_id: 1 };
    layout.split_pane(1, 2, SplitDirection::Vertical);
    layout.remove_pane(1);
    assert_eq!(layout.collect_pane_ids(), vec![2]);
}

#[test]
fn test_calculate_rects_vertical() {
    let layout = LayoutNode::Split {
        direction: SplitDirection::Vertical,
        ratio: 0.5,
        first: Box::new(LayoutNode::Leaf { pane_id: 1 }),
        second: Box::new(LayoutNode::Leaf { pane_id: 2 }),
    };
    let rects = layout.calculate_rects(Rect::new(0, 0, 100, 50));
    assert_eq!(rects.len(), 2);
    assert_eq!(rects[0], (1, Rect::new(0, 0, 50, 50)));
    assert_eq!(rects[1], (2, Rect::new(50, 0, 50, 50)));
}

#[test]
fn test_calculate_rects_horizontal() {
    let layout = LayoutNode::Split {
        direction: SplitDirection::Horizontal,
        ratio: 0.5,
        first: Box::new(LayoutNode::Leaf { pane_id: 1 }),
        second: Box::new(LayoutNode::Leaf { pane_id: 2 }),
    };
    let rects = layout.calculate_rects(Rect::new(0, 0, 100, 50));
    assert_eq!(rects.len(), 2);
    assert_eq!(rects[0], (1, Rect::new(0, 0, 100, 25)));
    assert_eq!(rects[1], (2, Rect::new(0, 25, 100, 25)));
}

#[test]
fn test_focus_cycling() {
    let ids = [1, 2, 3];
    assert_eq!(1 % ids.len(), 1);
    assert_eq!((2 + 1) % ids.len(), 0);
}

// ─── resolve_pane_ref_impl (Phase 3 Step 3.2) ────────────

fn mk_ids(ids: &[usize]) -> HashSet<usize> {
    ids.iter().copied().collect()
}

#[test]
fn resolve_focused_returns_focused_id_when_known() {
    let names = HashMap::new();
    let ids = mk_ids(&[1, 2, 3]);
    assert_eq!(
        resolve_pane_ref_impl(&PaneRef::Focused, &names, &ids, 2),
        Some(2)
    );
}

#[test]
fn resolve_focused_returns_none_when_focus_stale() {
    let names = HashMap::new();
    let ids = mk_ids(&[1, 3]);
    assert_eq!(
        resolve_pane_ref_impl(&PaneRef::Focused, &names, &ids, 2),
        None
    );
}

#[test]
fn resolve_by_id_returns_id_when_known() {
    let names = HashMap::new();
    let ids = mk_ids(&[1, 2, 3]);
    assert_eq!(
        resolve_pane_ref_impl(&PaneRef::Id(3), &names, &ids, 1),
        Some(3)
    );
}

#[test]
fn resolve_by_id_returns_none_when_unknown() {
    let names = HashMap::new();
    let ids = mk_ids(&[1, 2]);
    assert_eq!(
        resolve_pane_ref_impl(&PaneRef::Id(99), &names, &ids, 1),
        None
    );
}

#[test]
fn resolve_by_name_returns_id_when_registered() {
    let mut names = HashMap::new();
    names.insert("engineering".to_string(), 7);
    let ids = mk_ids(&[1, 7]);
    assert_eq!(
        resolve_pane_ref_impl(&PaneRef::Name("engineering".into()), &names, &ids, 1),
        Some(7)
    );
}

#[test]
fn resolve_by_name_returns_none_when_unregistered() {
    let names = HashMap::new();
    let ids = mk_ids(&[1, 7]);
    assert_eq!(
        resolve_pane_ref_impl(&PaneRef::Name("missing".into()), &names, &ids, 1),
        None
    );
}

#[test]
fn resolve_by_name_returns_none_when_pane_closed() {
    // Name still registered but the pane has been removed — the
    // dangling entry must not resolve to a ghost id.
    let mut names = HashMap::new();
    names.insert("engineering".to_string(), 7);
    let ids = mk_ids(&[1]); // 7 has been closed
    assert_eq!(
        resolve_pane_ref_impl(&PaneRef::Name("engineering".into()), &names, &ids, 1),
        None
    );
}

// ─── apply_layout integration (Phase 2 review fix) ────────
//
// These tests spawn real PTYs through App::new / Pane::new_with_cwd
// because apply_layout drives split_focused_pane, which unavoidably
// creates child shell processes. On a dev machine this costs a few
// milliseconds per pane; in CI it's measurable but acceptable for a
// handful of tests. Each test calls `app.shutdown()` at the end so
// the spawned shells don't linger.
