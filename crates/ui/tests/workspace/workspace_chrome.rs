use std::sync::Arc;

use ratatui::{Terminal, backend::TestBackend, layout::Rect, style::Color};
use ui::{
    RenderCapabilities, RenderContext, TundraTheme, WorkspaceAction, WorkspaceLayout,
    WorkspacePaneLayout, WorkspaceSplitMenu, WorkspaceSplitMenuViewModel, WorkspaceSplitMode,
    WorkspaceTaskbar, WorkspaceTaskbarAction, WorkspaceTaskbarItem, WorkspaceWindowAction,
    WorkspaceWindowButtonsState, automation::FrameCapture, render_workspace_pane_frame,
    render_workspace_split_menu, render_workspace_taskbar,
};
use workspace_protocol::{MIN_CONTENT_SIZE, SessionId};

const ID: SessionId = SessionId(8);

#[test]
fn complete_window_edges_and_actions_share_geometry_in_both_languages() {
    let available = Rect::new(0, 1, 108, 18);
    let terminal = Rect::new(0, 0, 108, 20);
    let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ascii-assets/assets");
    for (language, mode, names, restore, glyphs) in [
        (
            "en-US",
            ui::CellIconMode::Ascii,
            ["Minimize", "Split", "Close window"],
            "Restore",
            ["-", "|", "x"],
        ),
        (
            "zh-CN",
            ui::CellIconMode::Font,
            ["最小化", "分屏", "关闭窗口"],
            "还原",
            ["\u{e134}", "\u{e135}", "\u{e139}"],
        ),
    ] {
        ui::set_cell_icon_mode(mode);
        let language = i18n::LanguageSnapshot::load(&assets, language, 1)
            .unwrap()
            .snapshot;
        let _locale = i18n::enter_snapshot(Arc::new(language));
        let pane = WorkspaceLayout::new(
            terminal,
            available,
            &[(ID, MIN_CONTENT_SIZE)],
            &[ID],
            Some(ID),
            Some(&ui::WorkspaceTree::Window(ID)),
        )
        .panes[0];
        let context = RenderContext::from_theme(
            &TundraTheme::default(),
            Default::default(),
            RenderCapabilities::ansi(),
        );
        let mut terminal = Terminal::new(TestBackend::new(108, 20)).unwrap();
        let mut snapshot = None;
        terminal
            .draw(|frame| {
                let capture = FrameCapture::begin(frame.area(), "workspace");
                render_workspace_pane_frame(
                    frame,
                    &pane,
                    "日志 e\u{0301}",
                    true,
                    WorkspaceWindowButtonsState::default(),
                    &context,
                );
                snapshot = Some(capture.finish(frame.buffer_mut()));
            })
            .unwrap();
        let snapshot = snapshot.unwrap();
        let buffer = terminal.backend().buffer();
        for point in [(0, 5), (107, 5), (1, 18), (0, 1), (107, 18)] {
            assert!(!buffer[point].symbol().trim().is_empty(), "{point:?}");
        }
        assert!(snapshot.lines[1].contains("日志 e\u{0301}"));
        let actions = [
            WorkspaceWindowAction::Minimize,
            WorkspaceWindowAction::SplitMenu,
            WorkspaceWindowAction::Close,
        ];
        for (action, name) in actions.into_iter().zip(names) {
            let rect = pane.controls.rect(action);
            let node = snapshot
                .nodes
                .iter()
                .find(|node| node.id.ends_with(&action.id(ID)))
                .unwrap();
            assert_eq!(node.name, name);
            assert!(
                snapshot.lines[1].contains(name),
                "{name}: {}",
                snapshot.lines[1]
            );
            assert!(node.actionable);
            assert_eq!((node.rect.x, node.rect.width), (rect.x, rect.width));
            assert_eq!(pane.controls.hit_at((rect.x, rect.y)), Some(action));
            assert!(!matches!(buffer[(rect.x, rect.y)].bg, Color::Rgb(..)));
        }
        let mut restored = None;
        terminal
            .draw(|frame| {
                let capture = FrameCapture::begin(frame.area(), "workspace");
                render_workspace_pane_frame(
                    frame,
                    &pane,
                    "日志",
                    true,
                    WorkspaceWindowButtonsState {
                        close_disabled: true,
                        ..Default::default()
                    },
                    &context,
                );
                restored = Some(capture.finish(frame.buffer_mut()));
            })
            .unwrap();
        let restored = restored.unwrap();
        assert!(!restored.lines[1].contains(restore));
        assert!(restored.nodes.iter().any(|node| {
            node.id.ends_with(&WorkspaceWindowAction::Close.id(ID))
                && !node.enabled
                && !node.actionable
        }));

        let compact = WorkspacePaneLayout::for_blocked_window(ID, Rect::new(0, 1, 21, 3)).unwrap();
        let mut snapshot = None;
        terminal
            .draw(|frame| {
                let capture = FrameCapture::begin(frame.area(), "workspace");
                render_workspace_pane_frame(
                    frame,
                    &compact,
                    "日志",
                    true,
                    WorkspaceWindowButtonsState::default(),
                    &context,
                );
                snapshot = Some(capture.finish(frame.buffer_mut()));
            })
            .unwrap();
        let snapshot = snapshot.unwrap();
        for ((action, name), glyph) in actions.into_iter().zip(names).zip(glyphs) {
            let node = snapshot
                .nodes
                .iter()
                .find(|node| node.id.ends_with(&action.id(ID)))
                .unwrap();
            assert_eq!(node.name, name);
            assert_eq!(node.text.trim(), glyph);
            assert!(node.actionable);
        }
    }
    ui::set_cell_icon_mode(ui::CellIconMode::Ascii);
}

fn windows(count: u64) -> Vec<WorkspaceTaskbarItem> {
    (1..=count)
        .map(|id| WorkspaceTaskbarItem {
            id: SessionId(id),
            title: format!("{id} 编辑器 e\u{0301}"),
            focused: id == 1,
            minimized: id > 1,
            closing: id == count,
            unresponsive: id == 2,
        })
        .collect()
}

#[test]
fn taskbar_keeps_minimized_windows_reachable_and_leaves_neighboring_status_cells() {
    let windows = windows(10);
    let area = Rect::new(0, 4, 40, 1);
    let first = WorkspaceTaskbar::new(area, &windows, 0);
    assert!(
        first
            .hits
            .iter()
            .any(|(action, _)| *action == WorkspaceTaskbarAction::ScrollRight)
    );
    assert!(
        !first
            .hits
            .iter()
            .any(|(action, _)| *action == WorkspaceTaskbarAction::ScrollLeft)
    );
    let last = WorkspaceTaskbar::new(area, &windows, usize::MAX);
    assert_eq!(last.scroll, last.max_scroll);
    assert!(
        last.hits
            .iter()
            .any(|(action, _)| *action == WorkspaceTaskbarAction::Activate(SessionId(10)))
    );
    assert!(
        !last
            .hits
            .iter()
            .any(|(action, _)| *action == WorkspaceTaskbarAction::ScrollRight)
    );
    for (action, rect) in &first.hits {
        assert_eq!(rect.intersection(area), *rect);
        assert_eq!(first.hit_at((rect.x, rect.y)), Some(*action));
    }
    let mut terminal = Terminal::new(TestBackend::new(42, 5)).unwrap();
    let mut snapshot = None;
    terminal
        .draw(|frame| {
            frame.buffer_mut()[(40, 4)].set_symbol("X");
            let capture = FrameCapture::begin(frame.area(), "workspace");
            render_workspace_taskbar(
                frame,
                &first,
                &windows,
                None,
                None,
                &RenderContext::default(),
            );
            snapshot = Some(capture.finish(frame.buffer_mut()));
        })
        .unwrap();
    assert_eq!(terminal.backend().buffer()[(40, 4)].symbol(), "X");
    let snapshot = snapshot.unwrap();
    let active = snapshot
        .nodes
        .iter()
        .find(|node| node.id.ends_with("workspace.taskbar.1"))
        .unwrap();
    assert!(active.selected && active.actionable);
    let close = snapshot
        .nodes
        .iter()
        .find(|node| node.id.ends_with("workspace.taskbar.1.close"))
        .unwrap();
    assert!(close.actionable);
    let close_area = first
        .hits
        .iter()
        .find_map(|(action, area)| {
            (*action == WorkspaceTaskbarAction::Close(SessionId(1))).then_some(*area)
        })
        .unwrap();
    assert_eq!(
        terminal.backend().buffer()[(close_area.right() - 1, close_area.y)].symbol(),
        ui::cell_icon("nav", "close")
    );
    let minimized = snapshot
        .nodes
        .iter()
        .find(|node| node.id.ends_with("workspace.taskbar.2"))
        .unwrap();
    assert!(minimized.name.contains("Unresponsive"));
    assert!(minimized.actionable);
    assert!(
        last.hits
            .iter()
            .any(|(action, _)| *action == WorkspaceTaskbarAction::Close(SessionId(10)))
    );
}

#[test]
fn split_menu_is_anchored_and_disabled_actions_remain_visible() {
    let bounds = Rect::new(1, 2, 106, 16);
    let anchor = Rect::new(80, 1, 14, 1);
    let model = WorkspaceSplitMenuViewModel {
        window: ID,
        left_right: true,
        top_bottom: false,
        fullscreen: true,
        ..Default::default()
    };
    let menu = WorkspaceSplitMenu::new(bounds, anchor, &model);
    assert_eq!(menu.area.intersection(bounds), menu.area);
    assert_eq!(menu.area.y, anchor.bottom());
    assert!(
        !menu
            .hits
            .iter()
            .any(|(action, _)| *action == WorkspaceAction::SetMode(WorkspaceSplitMode::TopBottom))
    );
    for (action, rect) in &menu.hits {
        assert_eq!(rect.intersection(menu.area), *rect);
        assert_eq!(menu.hit_at((rect.x, rect.y)), Some(*action));
    }
    let mut terminal = Terminal::new(TestBackend::new(108, 20)).unwrap();
    let mut snapshot = None;
    terminal
        .draw(|frame| {
            let capture = FrameCapture::begin(frame.area(), "workspace");
            render_workspace_split_menu(frame, &menu, &model, &RenderContext::default());
            snapshot = Some(capture.finish(frame.buffer_mut()));
        })
        .unwrap();
    let snapshot = snapshot.unwrap();
    for (axis, enabled) in [
        (WorkspaceSplitMode::LeftRight, true),
        (WorkspaceSplitMode::TopBottom, false),
    ] {
        let node = snapshot
            .nodes
            .iter()
            .find(|node| node.id.ends_with(&WorkspaceAction::SetMode(axis).id()))
            .unwrap();
        assert_eq!(node.enabled, enabled);
        assert_eq!(node.actionable, enabled);
    }
    assert!(
        snapshot
            .nodes
            .iter()
            .any(|node| node.id.ends_with(&WorkspaceAction::Maximize(ID).id()) && node.actionable)
    );
}
