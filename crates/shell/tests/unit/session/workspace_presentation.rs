use super::close_tests::{connected_window, shell_state, window};
use super::*;
use ratatui::{Terminal, backend::TestBackend};

fn startup(requested: Instant) -> WindowStartup {
    WindowStartup {
        requested,
        accepted: None,
        starting_presented: false,
        app_started: None,
        init_queued: None,
        process_spawn_us: 0,
        guardian_ready_wait_us: 0,
        app_timings: None,
    }
}

fn workspace(width: u16, height: u16) -> (Workspace, SessionId) {
    let mut workspace = Workspace::new(None, false);
    let id = SessionId(1);
    workspace.windows.insert(id, window(Size::new(106, 16)));
    workspace.selection.open(id);
    relayout(&mut workspace, width, height);
    (workspace, id)
}

fn relayout(workspace: &mut Workspace, width: u16, height: u16) {
    workspace.relayout(
        Rect::new(0, 0, width, height),
        Rect::new(0, 1, width, height - 2),
        false,
    );
}

fn press(
    workspace: &mut Workspace,
    state: &mut ShellSession,
    point: CellPosition,
) -> Option<PointerTarget> {
    workspace
        .input(&InputEvent::mouse_down(PointerButton::Left, point), state)
        .unwrap();
    let target = workspace.capture.map(|capture| capture.target);
    workspace.cancel_capture();
    target
}

fn app_frame(workspace: &Workspace, id: SessionId, label: &str) -> WindowFrame {
    let pane = workspace
        .layout
        .panes
        .iter()
        .find(|pane| pane.id == id)
        .unwrap();
    let mut frame = WindowFrame::new(
        workspace.generation,
        Size::new(pane.content.width, pane.content.height),
    );
    for (cell, symbol) in frame.cells.iter_mut().zip(label.chars()) {
        cell.symbol = symbol.to_string();
    }
    frame.cells.last_mut().unwrap().symbol = "Z".into();
    frame.cursor = Some(workspace_protocol::Cursor {
        x: 0,
        y: 0,
        shape: CursorShape::SteadyBar,
    });
    frame.controls.push(workspace_protocol::ControlNode {
        id: "page/old-button".into(),
        parent: Some("page".into()),
        role: "button".into(),
        name: label.into(),
        text: String::new(),
        rect: workspace_protocol::Rect {
            x: 0,
            y: 0,
            width: 4,
            height: 1,
        },
        focused: true,
        selected: false,
        enabled: true,
        actionable: true,
        sensitive: false,
        actions: vec!["click".into()],
    });
    frame
}

fn render(
    workspace: &mut Workspace,
    terminal: &mut Terminal<TestBackend>,
    context: &ui::RenderContext,
    picker: Option<&ui::EditorImagePicker>,
    caches: &mut HashMap<SessionId, ui::TerminalImageCache>,
) -> shell_control::Snapshot {
    let mut snapshot = None;
    terminal
        .draw(|frame| {
            let capture = ui::automation::FrameCapture::begin(frame.area(), "workspace");
            let layout =
                ui::ShellFrameLayout::new(frame.area(), None, context).with_workspace_taskbar(true);
            workspace.render_panes(
                frame,
                layout,
                context,
                Some(&capture),
                false,
                picker,
                caches,
                &mut Vec::new(),
                false,
            );
            snapshot = Some(capture.finish(frame.buffer_mut()));
        })
        .unwrap();
    snapshot.unwrap()
}

#[test]
fn first_window_waits_over_100ms_before_showing_starting() {
    let requested = Instant::now();
    let mut window = window(Size::new(106, 16));
    window.startup = Some(startup(requested));
    for millis in [0, 99, 100] {
        assert_eq!(
            window.initial_placeholder(requested + Duration::from_millis(millis)),
            WorkspacePanePlaceholder::Pending,
        );
    }
    assert_eq!(
        window.initial_placeholder(requested + Duration::from_millis(101)),
        WorkspacePanePlaceholder::Starting,
    );
}

#[test]
fn starting_deadlines_follow_color_capability_and_reduced_motion() {
    for (color, reduced_motion, interval) in [
        (ui::ColorCapability::TrueColor, false, Some(50)),
        (ui::ColorCapability::Ansi, false, Some(80)),
        (ui::ColorCapability::TrueColor, true, None),
    ] {
        let (mut workspace, id) = workspace(120, 36);
        workspace.windows.get_mut(&id).unwrap().startup =
            Some(startup(Instant::now() - Duration::from_secs(1)));
        let mut context = ui::RenderContext::default();
        context.motion.reduced_motion = reduced_motion;
        context.capabilities.color = color;
        let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
        let before = Instant::now();
        let snapshot = render(
            &mut workspace,
            &mut terminal,
            &context,
            None,
            &mut HashMap::new(),
        );
        let after = Instant::now();
        let starting = snapshot
            .nodes
            .iter()
            .find(|node| node.id.ends_with("workspace.starting"))
            .unwrap();
        assert!(!starting.actionable);
        assert_eq!(workspace.animating(), interval.is_some());
        match interval {
            Some(ms) => {
                let deadline = workspace.placeholder_redraw_at.unwrap();
                assert!(deadline >= before + Duration::from_millis(ms));
                assert!(deadline <= after + Duration::from_millis(ms));
                assert!(!workspace.placeholder_redraw_due(deadline - Duration::from_nanos(1)));
                assert!(workspace.placeholder_redraw_due(deadline));
                assert!(!workspace.placeholder_redraw_due(deadline));
            }
            None => assert!(workspace.placeholder_redraw_at.is_none()),
        }
        assert!(
            !workspace.windows[&id]
                .startup
                .as_ref()
                .unwrap()
                .starting_presented
        );
        workspace.did_present(&shell_state(), 0);
        assert!(
            workspace.windows[&id]
                .startup
                .as_ref()
                .unwrap()
                .starting_presented
        );
    }
}

#[test]
fn initial_background_and_starting_are_replaced_on_the_first_frame() {
    let (mut workspace, id) = workspace(120, 36);
    let requested = Instant::now() + Duration::from_secs(1);
    workspace.windows.get_mut(&id).unwrap().startup = Some(startup(requested));
    let context = ui::RenderContext::default();
    let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
    let mut caches = HashMap::new();
    let snapshot = render(&mut workspace, &mut terminal, &context, None, &mut caches);
    assert!(
        !snapshot
            .nodes
            .iter()
            .any(|node| node.id.ends_with("workspace.starting"))
    );
    assert_eq!(
        workspace.placeholder_redraw_at,
        Some(requested + STARTING_DELAY + Duration::from_nanos(1))
    );
    let pane = workspace.layout.panes[0];
    assert_eq!(
        terminal.backend().buffer()[(pane.content.x, pane.content.y)].symbol(),
        " "
    );

    workspace
        .windows
        .get_mut(&id)
        .unwrap()
        .startup
        .as_mut()
        .unwrap()
        .requested = Instant::now() - Duration::from_secs(1);
    render(&mut workspace, &mut terminal, &context, None, &mut caches);
    workspace.did_present(&shell_state(), 0);
    let frame = app_frame(&workspace, id, "READY");
    let mut state = shell_state();
    workspace
        .receive(id, AppToShell::Frame(frame), &mut state)
        .unwrap();
    let snapshot = render(&mut workspace, &mut terminal, &context, None, &mut caches);
    assert!(
        !snapshot
            .nodes
            .iter()
            .any(|node| node.id.ends_with("workspace.starting"))
    );
    assert_eq!(
        terminal.backend().buffer()[(pane.content.x, pane.content.y)].symbol(),
        "R"
    );
    assert!(workspace.placeholder_redraw_at.is_none());
    assert!(workspace.windows[&id].presented.is_none());
    workspace.did_present(&state, 0);
    assert!(workspace.windows[&id].presented.is_some());
    assert!(workspace.windows[&id].startup.is_none());
    assert!(!workspace.animating());
}

#[test]
fn failure_and_close_states_bypass_the_starting_delay() {
    for placeholder in [
        WorkspacePanePlaceholder::Unresponsive,
        WorkspacePanePlaceholder::Closing,
        WorkspacePanePlaceholder::Unavailable,
    ] {
        let (mut workspace, id) = workspace(120, 36);
        let window = workspace.windows.get_mut(&id).unwrap();
        window.startup = Some(startup(Instant::now() + Duration::from_secs(1)));
        let text = match placeholder {
            WorkspacePanePlaceholder::Unresponsive => {
                window.state = ProcessState::Unresponsive;
                i18n::tr!("ui-workspace-unresponsive")
            }
            WorkspacePanePlaceholder::Closing => {
                window.close = CloseProgress::Requested(Instant::now());
                i18n::tr!("ui-workspace-closing")
            }
            WorkspacePanePlaceholder::Unavailable => {
                window.launch_error = true;
                i18n::tr!("ui-components-unavailable")
            }
            _ => unreachable!(),
        };
        let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
        let snapshot = render(
            &mut workspace,
            &mut terminal,
            &ui::RenderContext::default(),
            None,
            &mut HashMap::new(),
        );
        assert!(snapshot.lines.join("\n").contains(&text));
        assert!(
            !snapshot
                .nodes
                .iter()
                .any(|node| node.id.ends_with("workspace.starting"))
        );
        assert!(workspace.placeholder_redraw_at.is_none());
    }
}

#[test]
fn panes_update_independently_and_keep_content_through_fullscreen_and_restore() {
    let (mut workspace, first) = workspace(217, 39);
    let second = SessionId(2);
    workspace.windows.insert(second, window(Size::new(106, 16)));
    workspace
        .selection
        .split(first, WorkspaceSplitMode::LeftRight);
    workspace.selection.open(second);
    relayout(&mut workspace, 217, 39);
    let context = ui::RenderContext::default();
    let mut terminal = Terminal::new(TestBackend::new(217, 39)).unwrap();
    let mut caches = HashMap::new();
    let mut state = shell_state();
    for (id, label) in [(first, "FIRST"), (second, "SECOND")] {
        let frame = app_frame(&workspace, id, label);
        workspace
            .receive(id, AppToShell::Frame(frame), &mut state)
            .unwrap();
    }
    render(&mut workspace, &mut terminal, &context, None, &mut caches);
    workspace.did_present(&state, 0);
    relayout(&mut workspace, 239, 41);
    terminal.backend_mut().resize(239, 41);
    let frame = app_frame(&workspace, first, "FRESH");
    workspace
        .receive(first, AppToShell::Frame(frame), &mut state)
        .unwrap();
    let snapshot = render(&mut workspace, &mut terminal, &context, None, &mut caches);
    let text = snapshot.lines.join("\n");
    assert!(text.contains("FRESH") && text.contains("SECOND"));
    assert!(!workspace.external_snapshot_ready());
    let second_button = snapshot
        .nodes
        .iter()
        .find(|node| node.name == "SECOND")
        .unwrap();
    assert!(!second_button.actionable);
    workspace.did_present(&state, 0);

    workspace.selection.fullscreen(first);
    relayout(&mut workspace, 239, 41);
    let snapshot = render(&mut workspace, &mut terminal, &context, None, &mut caches);
    assert!(snapshot.lines.join("\n").contains("FRESH"));
    workspace.selection.restore(second, &workspace.layout);
    relayout(&mut workspace, 239, 41);
    let snapshot = render(&mut workspace, &mut terminal, &context, None, &mut caches);
    assert!(snapshot.lines.join("\n").contains("SECOND"));
    assert!(
        !snapshot
            .nodes
            .iter()
            .any(|node| node.id.ends_with("workspace.starting"))
    );
}

#[test]
#[cfg_attr(not(feature = "native-tests"), ignore = "requires local socket access")]
fn resize_retains_only_the_last_presented_complete_frame_and_freezes_its_controls() {
    let (mut workspace, id) = workspace(120, 36);
    let (connected, _peer, _listener) = connected_window(Size::new(106, 16));
    workspace.windows.get_mut(&id).unwrap().stream = connected.stream;
    let context = ui::RenderContext::default();
    let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
    let mut caches = HashMap::new();
    let mut state = shell_state();
    let frame = app_frame(&workspace, id, "OLD");
    workspace
        .receive(id, AppToShell::Frame(frame), &mut state)
        .unwrap();
    render(&mut workspace, &mut terminal, &context, None, &mut caches);
    workspace.did_present(&state, 0);
    let original = Arc::clone(workspace.windows[&id].presented.as_ref().unwrap());
    let old_pane = workspace.layout.panes[0];
    workspace
        .input(
            &InputEvent::mouse_down(
                PointerButton::Left,
                (old_pane.content.x, old_pane.content.y),
            ),
            &mut state,
        )
        .unwrap();
    assert!(workspace.capture.is_some());
    assert!(workspace.windows[&id].queued.iter().any(|message| matches!(
        message,
        ShellToApp::Input { layout, .. } if *layout == original.layout
    )));
    let mut newer = app_frame(&workspace, id, "UNPRESENTED");
    newer.animation_running = true;
    workspace
        .receive(id, AppToShell::Frame(newer), &mut state)
        .unwrap();

    relayout(&mut workspace, 108, 20);
    assert!(workspace.capture.is_none());
    assert_eq!(workspace.suppress_release, Some(PointerButton::Left));
    assert!(workspace.windows[&id].queued.iter().any(|message| matches!(
        message,
        ShellToApp::CancelPointer { layout } if *layout == original.layout
    )));
    terminal.backend_mut().resize(108, 20);
    let snapshot = render(&mut workspace, &mut terminal, &context, None, &mut caches);
    let pane = workspace.layout.panes[0];
    assert_eq!(
        terminal.backend().buffer()[(pane.content.x, pane.content.y)].symbol(),
        "O"
    );
    let old = snapshot
        .nodes
        .iter()
        .find(|node| node.id == "page/old-button")
        .unwrap();
    assert!(!old.actionable && !old.focused && old.actions.is_empty());
    assert_eq!(
        press(&mut workspace, &mut state, (pane.content.x, pane.content.y)),
        None
    );
    assert_eq!(
        press(
            &mut workspace,
            &mut state,
            (pane.controls.close.x, pane.controls.close.y)
        ),
        Some(PointerTarget::WindowButton(
            id,
            WorkspaceWindowAction::Close
        ))
    );
    assert_eq!(workspace.cursor_shape(), None);
    assert!(!workspace.animating());
    assert!(!workspace.external_snapshot_ready());
    let delayed = app_frame(&workspace, id, "DELAYED");
    workspace.did_present(&state, 0);
    assert!(Arc::ptr_eq(
        &original,
        workspace.windows[&id].presented.as_ref().unwrap()
    ));

    relayout(&mut workspace, 130, 40);
    terminal.backend_mut().resize(130, 40);
    assert!(
        !workspace
            .receive(id, AppToShell::Frame(delayed), &mut state)
            .unwrap()
    );
    render(&mut workspace, &mut terminal, &context, None, &mut caches);
    let pane = workspace.layout.panes[0];
    let old_corner = (
        pane.content.x + original.size.width - 1,
        pane.content.y + original.size.height - 1,
    );
    assert_eq!(terminal.backend().buffer()[old_corner].symbol(), "Z");
    let new_corner = (pane.content.right() - 1, pane.content.bottom() - 1);
    assert_eq!(terminal.backend().buffer()[new_corner].symbol(), " ");
    assert_eq!(
        terminal.backend().buffer()[new_corner].bg,
        context.theme.canvas
    );

    let mut wrong_size = app_frame(&workspace, id, "WRONG");
    wrong_size.size.width -= 1;
    wrong_size.cells = vec![workspace_protocol::Cell::default(); wrong_size.size.area()];
    assert!(
        !workspace
            .receive(id, AppToShell::Frame(wrong_size), &mut state)
            .unwrap()
    );
    let latest = app_frame(&workspace, id, "CURRENT");
    workspace
        .receive(id, AppToShell::Frame(latest), &mut state)
        .unwrap();
    render(&mut workspace, &mut terminal, &context, None, &mut caches);
    assert_eq!(
        terminal.backend().buffer()[(pane.content.x, pane.content.y)].symbol(),
        "C"
    );
    assert_eq!(
        press(&mut workspace, &mut state, (pane.content.x, pane.content.y)),
        None
    );
    workspace.did_present(&state, 0);
    assert_eq!(
        press(&mut workspace, &mut state, (pane.content.x, pane.content.y)),
        Some(PointerTarget::Pane(id))
    );
    assert!(workspace.external_snapshot_ready());
}

#[test]
fn split_cancel_keeps_the_complete_frame_and_native_image() {
    let (mut workspace, id) = workspace(217, 39);
    let context = ui::RenderContext::default();
    let mut terminal = Terminal::new(TestBackend::new(217, 39)).unwrap();
    let mut caches = HashMap::new();
    let mut state = shell_state();
    let mut frame = app_frame(&workspace, id, "ORIGINAL");
    let image_area = workspace_protocol::Rect {
        x: 104,
        y: 1,
        width: 4,
        height: 1,
    };
    frame.images.push(workspace_protocol::Image {
        image_id: 1,
        width: 8,
        height: 2,
        rgba: Arc::from([25, 100, 200, 255].repeat(16)),
        source: [0, 0, 8, 2],
        padding: [0; 4],
        cell_size: [2, 2],
        area: image_area,
        visible: vec![image_area],
        z_index: 0,
    });
    workspace
        .receive(id, AppToShell::Frame(frame), &mut state)
        .unwrap();
    let probe = ui::TerminalGraphicsProbe::from_terminal_capabilities(
        ui::EditorGraphicsProtocol::Iterm2,
        2,
        2,
        false,
        false,
    );
    render(
        &mut workspace,
        &mut terminal,
        &context,
        probe.picker(),
        &mut caches,
    );
    workspace.did_present(&state, 0);
    let original = Arc::clone(workspace.windows[&id].presented.as_ref().unwrap());
    workspace.begin_split(id, WorkspaceSplitMode::LeftRight, &state);
    render(
        &mut workspace,
        &mut terminal,
        &context,
        probe.picker(),
        &mut caches,
    );
    let pane = workspace
        .layout
        .panes
        .iter()
        .find(|pane| pane.id == id)
        .unwrap();
    assert_eq!(pane.content.width, 106);
    assert!(
        terminal.backend().buffer()[(pane.content.x + 104, pane.content.y + 1)]
            .symbol()
            .contains("\x1b]1337;File=")
    );
    assert!(caches.contains_key(&id));
    workspace.did_present(&state, 0);
    assert!(Arc::ptr_eq(
        &original,
        workspace.windows[&id].presented.as_ref().unwrap()
    ));
    workspace.cancel_chooser();
    relayout(&mut workspace, 217, 39);
    render(
        &mut workspace,
        &mut terminal,
        &context,
        probe.picker(),
        &mut caches,
    );
    let pane = workspace.layout.panes[0];
    assert_eq!(
        terminal.backend().buffer()[(pane.content.right() - 1, pane.content.bottom() - 1)].symbol(),
        "Z"
    );
    assert!(!workspace.animating());
}
