use ratatui::layout::Rect;
use ui::{WorkspaceLayout, WorkspacePaneLayout, WorkspaceSplitMode as Axis, WorkspaceTree};
use workspace_protocol::{MIN_CONTENT_SIZE, SessionId, Size};

fn layout(width: u16, height: u16, tree: &WorkspaceTree, focused: SessionId) -> WorkspaceLayout {
    WorkspaceLayout::new(
        Rect::new(0, 0, width, height),
        Rect::new(0, 1, width, height - 2),
        &[],
        &tree.ids(),
        Some(focused),
        Some(tree),
    )
}

#[test]
fn capacity_is_bounded_by_two_rows_and_two_columns_and_includes_all_edges() {
    for (width, height, capacity) in [
        (108, 20, 1),
        (216, 20, 1),
        (217, 20, 2),
        (108, 38, 1),
        (108, 39, 2),
        (217, 39, 4),
        (1000, 500, 4),
    ] {
        let tree = WorkspaceTree::Window(SessionId(1));
        assert_eq!(
            layout(width, height, &tree, SessionId(1)).capacity,
            capacity
        );
    }
    let tree = WorkspaceTree::Window(SessionId(1));
    let pane = layout(108, 20, &tree, SessionId(1)).panes[0];
    assert_eq!(pane.content, Rect::new(1, 2, 106, 16));
    assert_eq!(pane.controls.close.y, pane.area.y);
}

#[test]
fn incremental_split_keeps_the_other_pane_and_stops_at_four() {
    let (a, b, c, d) = (SessionId(1), SessionId(2), SessionId(3), SessionId(4));
    for (first, second) in [
        (Axis::LeftRight, Axis::TopBottom),
        (Axis::TopBottom, Axis::LeftRight),
    ] {
        let mut tree = WorkspaceTree::Window(a);
        assert!(tree.split(a, b, first));
        assert!(!tree.split(a, SessionId(5), first));
        let before = layout(217, 39, &tree, a);
        assert!(tree.split(a, c, second));
        let after = layout(217, 39, &tree, c);
        assert_eq!(
            before.panes.iter().find(|pane| pane.id == b),
            after.panes.iter().find(|pane| pane.id == b)
        );
        assert!(tree.split(b, d, second));
        assert!(!tree.split(d, SessionId(5), first));
        assert!(!tree.split(d, SessionId(5), second));
        let grid = layout(217, 39, &tree, d);
        assert_eq!(grid.panes.len(), 4);
        for pane in &grid.panes {
            assert_eq!((pane.content.width, pane.content.height), (106, 16));
        }
        for (i, pane) in grid.panes.iter().enumerate() {
            for other in grid.panes.iter().skip(i + 1) {
                assert!(pane.area.intersection(other.area).is_empty());
            }
        }
    }
}

#[test]
fn cancel_removes_only_the_pending_leaf_and_minimize_can_restore_its_position() {
    let (a, b, c) = (SessionId(1), SessionId(2), SessionId(3));
    let mut tree = WorkspaceTree::Window(a);
    tree.split(a, b, Axis::LeftRight);
    let previous = tree.clone();
    tree.split(a, c, Axis::TopBottom);
    assert_eq!(tree.without(c), Some(previous.clone()));
    let minimized = previous.without(b).unwrap();
    assert_eq!(minimized.restore(&previous, b), Some(previous));
}

#[test]
fn resize_hides_panes_without_discarding_topology_and_keeps_the_focused_window() {
    let (a, b, c, d) = (SessionId(1), SessionId(2), SessionId(3), SessionId(4));
    let mut tree = WorkspaceTree::Window(a);
    tree.split(a, b, Axis::LeftRight);
    tree.split(a, c, Axis::TopBottom);
    tree.split(b, d, Axis::TopBottom);
    let full = layout(217, 39, &tree, d);
    let small = layout(108, 20, &tree, d);
    assert_eq!(small.panes.len(), 1);
    assert_eq!(small.panes[0].id, d);
    assert_eq!(layout(217, 39, &tree, d), full);
    let wide = layout(217, 20, &tree, d);
    assert_eq!(wide.panes.len(), 2);
    assert!(wide.panes.iter().any(|pane| pane.id == d));
}

#[test]
fn background_minimums_still_block_content_and_leave_window_controls_available() {
    let (a, background) = (SessionId(1), SessionId(2));
    let tree = WorkspaceTree::Window(a);
    let result = WorkspaceLayout::new(
        Rect::new(0, 0, 108, 20),
        Rect::new(0, 1, 108, 18),
        &[(a, MIN_CONTENT_SIZE), (background, Size::new(130, 25))],
        &[a],
        Some(a),
        Some(&tree),
    );
    assert!(result.blocked);
    assert_eq!(result.size_needed, Size::new(132, 29));
    assert!(WorkspacePaneLayout::for_blocked_window(a, result.available).is_some());
}
