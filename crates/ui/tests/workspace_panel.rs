use std::sync::Arc;

use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use ui::{
    ColorCapability, RenderCapabilities, RenderContext, TundraTheme, WorkspaceAction,
    WorkspaceCloseWait, WorkspacePanePlaceholder, WorkspacePanel, WorkspacePanelViewModel,
    WorkspaceWindowViewModel, automation::FrameCapture, render_workspace_close_wait,
    render_workspace_pane_placeholder, render_workspace_panel,
};
use workspace_protocol::{SessionId, Size};

const ACTIVE: SessionId = SessionId(1);
const BACKGROUND: SessionId = SessionId(2);

fn model() -> WorkspacePanelViewModel {
    WorkspacePanelViewModel {
        windows: vec![
            WorkspaceWindowViewModel {
                id: ACTIVE,
                title: "Diagnostics".into(),
                minimum_size: Size::new(108, 18),
                visible: true,
                unresponsive: false,
                closing: false,
            },
            WorkspaceWindowViewModel {
                id: BACKGROUND,
                title: "文本 e\u{0301} 窗口".repeat(12),
                minimum_size: Size::new(160, 24),
                visible: false,
                unresponsive: true,
                closing: false,
            },
        ],
        focused: Some(ACTIVE),
        selected: Some(BACKGROUND),
        query: String::new(),
        editing: Default::default(),
        size_needed: Size::new(160, 26),
        capacity: 0,
        blocked: true,
        scroll: 0,
        action_focus: Some(WorkspaceAction::Select(BACKGROUND)),
        hovered: None,
        pressed: None,
    }
}

#[test]
fn size_blocked_panel_keeps_background_windows_and_danger_action_accessible() {
    let model = model();
    let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ascii-assets/assets");
    for (language, required, unresponsive, background) in [
        ("en-US", "160×26", "Unresponsive", "In background"),
        ("zh-CN", "160×26", "无响应", "后台"),
    ] {
        let snapshot = i18n::LanguageSnapshot::load(&assets, language, 1)
            .unwrap()
            .snapshot;
        let _locale = i18n::enter_snapshot(Arc::new(snapshot));
        for color in [ColorCapability::TrueColor, ColorCapability::Ansi] {
            let context = RenderContext::from_theme(
                &TundraTheme::default(),
                Default::default(),
                RenderCapabilities {
                    color,
                    ..Default::default()
                },
            );
            let area = Rect::new(0, 1, 108, 16);
            let panel = WorkspacePanel::new(area, &model);
            let mut terminal = Terminal::new(TestBackend::new(108, 18)).unwrap();
            let mut snapshot = None;
            terminal
                .draw(|frame| {
                    let capture = FrameCapture::begin(frame.area(), "workspace");
                    render_workspace_panel(frame, &panel, &model, &context);
                    snapshot = Some(capture.finish(frame.buffer_mut()));
                })
                .unwrap();
            let snapshot = snapshot.unwrap();
            let text = snapshot.lines.join("\n");
            assert!(text.contains(required), "{text}");
            assert!(text.contains(unresponsive), "{text}");
            assert!(text.contains(background), "{text}");

            for action in [
                WorkspaceAction::Select(BACKGROUND),
                WorkspaceAction::Close(BACKGROUND),
                WorkspaceAction::ForceKill(BACKGROUND),
            ] {
                let (.., region) = panel.hits.iter().find(|(hit, _)| *hit == action).unwrap();
                let coords = (region.x + region.width / 2, region.y);
                assert_eq!(panel.hit_at(coords), Some(action));
                let node = snapshot
                    .nodes
                    .iter()
                    .find(|node| node.id.ends_with(&action.id()))
                    .unwrap();
                assert!(node.actionable);
                assert_eq!((node.rect.x, node.rect.y), (region.x, region.y));
                if action == WorkspaceAction::ForceKill(BACKGROUND) {
                    let cell = &terminal.backend().buffer()[coords];
                    assert_eq!(cell.fg, context.theme.danger);
                }
            }
        }
    }
}

#[test]
fn close_wait_renders_wait_and_force_kill_with_shared_hit_regions() {
    let area = Rect::new(0, 1, 108, 18);
    let mut terminal = Terminal::new(TestBackend::new(108, 20)).unwrap();
    for (size_blocked, phrase) in [
        (false, "Cleanup is still running."),
        (true, "This window needs a larger terminal"),
    ] {
        let wait = WorkspaceCloseWait::new(area, "编辑器 e\u{0301}", BACKGROUND, size_blocked);
        let mut snapshot = None;
        terminal
            .draw(|frame| {
                let capture = FrameCapture::begin(frame.area(), "workspace");
                render_workspace_close_wait(
                    frame,
                    &wait,
                    None,
                    None,
                    None,
                    &RenderContext::default(),
                );
                snapshot = Some(capture.finish(frame.buffer_mut()));
            })
            .unwrap();
        let snapshot = snapshot.unwrap();
        assert!(snapshot.lines.join("\n").contains(phrase));
        for action in [
            WorkspaceAction::Dismiss,
            WorkspaceAction::ForceKill(BACKGROUND),
        ] {
            let (_, area) = wait.hits.iter().find(|(hit, _)| *hit == action).unwrap();
            assert_eq!(wait.hit_at((area.x, area.y)), Some(action));
            let node = snapshot
                .nodes
                .iter()
                .find(|node| node.id.ends_with(&action.id()))
                .unwrap();
            assert_eq!(node.focused, action == WorkspaceAction::Dismiss);
        }
    }
}

#[test]
fn starting_is_localized_and_pending_has_only_the_canvas() {
    let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ascii-assets/assets");
    for language in ["en-US", "zh-CN"] {
        let snapshot = i18n::LanguageSnapshot::load(&assets, language, 1)
            .unwrap()
            .snapshot;
        let _locale = i18n::enter_snapshot(Arc::new(snapshot));
        for color in [ColorCapability::TrueColor, ColorCapability::Ansi] {
            let mut context = RenderContext::default();
            context.capabilities.color = color;
            context.motion.reduced_motion = true;
            let mut terminal = Terminal::new(TestBackend::new(30, 5)).unwrap();
            let area = Rect::new(1, 1, 28, 3);
            let mut snapshot = None;
            terminal
                .draw(|frame| {
                    let capture = FrameCapture::begin(frame.area(), "workspace");
                    render_workspace_pane_placeholder(
                        frame,
                        area,
                        WorkspacePanePlaceholder::Starting,
                        &context,
                    );
                    snapshot = Some(capture.finish(frame.buffer_mut()));
                })
                .unwrap();
            let snapshot = snapshot.unwrap();
            let starting = snapshot
                .nodes
                .iter()
                .find(|node| node.id.ends_with("workspace.starting"))
                .unwrap();
            assert_eq!(starting.name, i18n::tr!("ui-workspace-starting"));
            assert!(!starting.actionable && starting.actions.is_empty());
            assert!(snapshot.lines.join("\n").contains('⋯'));

            terminal
                .draw(|frame| {
                    render_workspace_pane_placeholder(
                        frame,
                        area,
                        WorkspacePanePlaceholder::Pending,
                        &context,
                    );
                })
                .unwrap();
            for y in area.y..area.bottom() {
                for x in area.x..area.right() {
                    let cell = &terminal.backend().buffer()[(x, y)];
                    assert_eq!(cell.symbol(), " ");
                    assert_eq!(cell.bg, context.theme.canvas);
                }
            }
        }
    }
}
