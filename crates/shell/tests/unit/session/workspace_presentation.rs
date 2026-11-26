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

#[test]
fn focused_package_window_opens_an_editable_draft_in_the_shell() {
    let (mut workspace, id) = workspace(108, 20);
    let mut shell = shell_state();
    shell.app.dispatch_at(
        app::AppCommand::SetAuthSession(Some(AuthSession {
            source: identity::IdentitySource::SystemCurrentProcess,
            session_id: "package".into(),
            username: "alice".into(),
            user_id: "alice".into(),
            role: UserRole::User,
            started_at_epoch_ms: 1,
        })),
        Instant::now(),
    );
    let dir = tempfile::tempdir().unwrap();
    shell.storage_manager = Some(
        StorageManager::open(platform::build_binary_dir_app_paths(dir.path()).unwrap())
            .unwrap()
            .manager,
    );
    let mut package = shell.clone();
    package.window_owner = super::super::window_launch::WindowOwner::App(
        super::super::window_launch::WindowApplication::Management(ManagementKind::Packages),
    );
    package.open_package_launcher_form("Demo".into(), "/usr/bin/demo".into());
    let request = serde_json::to_value(package.window_launches.pop_front().unwrap()).unwrap();
    workspace
        .receive(id, AppToShell::OpenApp(request.clone()), &mut shell)
        .unwrap();
    assert!(shell.pages.launcher.launcher_run_settings.is_none());
    workspace.windows.get_mut(&id).unwrap().launch =
        WindowLaunch::Management(WindowLaunch::management_query(ManagementKind::Packages));
    workspace
        .receive(id, AppToShell::OpenApp(request), &mut shell)
        .unwrap();
    let form = shell.pages.launcher.launcher_run_settings.as_ref().unwrap();
    assert_eq!(form.name.value(), "Demo");
    assert_eq!(form.arguments.value(), "/usr/bin/demo");
    assert!(
        shell
            .storage_manager
            .as_ref()
            .unwrap()
            .load_config()
            .unwrap()
            .launcher
            .entries
            .is_empty()
    );
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
    let cells = Arc::make_mut(&mut frame.cells);
    for (cell, symbol) in cells.iter_mut().zip(label.chars()) {
        cell.symbol = symbol.to_string().into();
    }
    cells.last_mut().unwrap().symbol = "Z".into();
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

#[test]
fn only_hover_within_the_current_front_pane_skips_shell_redraw() {
    let (mut workspace, id) = workspace(120, 36);
    let pane = &workspace.layout.panes[0];
    let inside = (pane.content.x, pane.content.y);
    let next = (inside.0 + 1, inside.1);
    let outside = (pane.area.x, pane.area.y);
    let frame = Arc::new(app_frame(&workspace, id, "current frame"));
    workspace.windows.get_mut(&id).unwrap().presented = Some(frame);

    let move_inside = InputEvent::mouse_moved(next);
    assert!(!workspace.is_same_pane_hover(&move_inside));
    workspace.last_pointer = Some(inside);
    assert!(workspace.is_same_pane_hover(&move_inside));
    assert!(!workspace.is_same_pane_hover(&InputEvent::mouse_moved(outside)));
    assert!(!workspace.is_same_pane_hover(&InputEvent::mouse_down(PointerButton::Left, next)));

    workspace.split_window = Some(id);
    assert!(!workspace.is_same_pane_hover(&move_inside));
    workspace.split_window = None;
    workspace.invalidate_layout();
    assert!(!workspace.is_same_pane_hover(&move_inside));
}

#[test]
fn workspace_back_follows_the_focused_pane_and_pending_split_menu() {
    let (mut workspace, first) = workspace(240, 40);
    let second = SessionId(2);
    workspace.windows.insert(second, window(Size::new(106, 16)));
    assert!(
        workspace
            .selection
            .split(first, WorkspaceSplitMode::LeftRight)
    );
    workspace.selection.open(second);
    workspace.selection.focus(first);
    relayout(&mut workspace, 240, 40);

    let mut first_frame = app_frame(&workspace, first, "previous page");
    first_frame.back_action = Some(BackAction::PreviousPage);
    workspace.windows.get_mut(&first).unwrap().frame = Some(Arc::new(first_frame));
    assert_eq!(workspace.back_action(), Some(BackAction::PreviousPage));

    workspace.selection.focus(second);
    relayout(&mut workspace, 240, 40);
    let second_frame = app_frame(&workspace, second, "tool root");
    workspace.windows.get_mut(&second).unwrap().frame = Some(Arc::new(second_frame));
    assert_eq!(workspace.back_action(), None);

    let windows_before = workspace.selection.visible();
    let mut state = shell_state();
    while state.notification_dismiss_active_modal_without_response() {}
    state.terminal_size = (240, 40);
    state.refresh_hit_map();
    for pointer in [true, false] {
        workspace.split_window = Some(second);
        let layout = ui::ShellFrameLayout::new(
            Rect::new(0, 0, 240, 40),
            None,
            &ui::RenderContext::default(),
        )
        .with_back_button(true, Some(BackAction::Cancel));
        workspace.remember_chrome(layout, None);
        assert_eq!(workspace.back_action(), Some(BackAction::Cancel));
        if pointer {
            let area = layout.back_button.unwrap();
            let point = (area.x, area.y);
            assert!(
                workspace
                    .input(
                        &InputEvent::mouse_down(PointerButton::Left, point),
                        &mut state
                    )
                    .unwrap()
            );
            assert!(
                !workspace
                    .input(
                        &InputEvent::mouse_drag(PointerButton::Left, (0, 0)),
                        &mut state
                    )
                    .unwrap()
            );
            assert!(
                workspace
                    .input(
                        &InputEvent::mouse_up(PointerButton::Left, point),
                        &mut state
                    )
                    .unwrap()
            );
            assert!(workspace.split_window.is_some());
            assert!(
                workspace
                    .input(
                        &InputEvent::mouse_down(PointerButton::Left, point),
                        &mut state
                    )
                    .unwrap()
            );
            assert!(workspace.split_window.is_some());
            assert!(
                workspace
                    .input(
                        &InputEvent::mouse_up(PointerButton::Left, point),
                        &mut state
                    )
                    .unwrap()
            );
        } else {
            assert!(
                workspace
                    .input(&InputEvent::key(InputKey::Escape), &mut state)
                    .unwrap()
            );
        }
        assert!(workspace.split_window.is_none());
        assert_eq!(workspace.selection.visible(), windows_before);
        assert_eq!(workspace.selection.focused(), Some(second));
    }
}

#[test]
fn power_title_suspends_the_workspace_split_menu_and_restores_it_after_cancel() {
    let (mut workspace, id) = workspace(240, 40);
    let mut state = shell_state();
    while state.notification_dismiss_active_modal_without_response() {}
    state.terminal_size = (240, 40);
    state.refresh_hit_map();
    let mut compositor = ScreenCompositor::default();
    compositor.capture_enabled = true;
    let mut terminal = Terminal::new(TestBackend::new(240, 40)).unwrap();
    let host = CommandLineHost::new(default_editor_watchdog().unwrap());
    let mut draw = |state: &mut ShellSession, workspace: &mut Workspace| {
        let context = ui::RenderContext {
            motion: ui::MotionFrame::reduced(Duration::ZERO),
            ..Default::default()
        };
        let prepared = compositor.prepare(
            state,
            &host,
            Instant::now(),
            Default::default(),
            context,
            None,
        );
        terminal
            .draw(|frame| {
                compositor.render(frame, state, &prepared, None, Some(workspace));
            })
            .unwrap();
        compositor.snapshot.clone().unwrap()
    };
    workspace.split_window = Some(id);
    let snapshot = draw(&mut state, &mut workspace);
    let title = snapshot
        .nodes
        .iter()
        .find(|node| node.id == "chrome/shell.power")
        .unwrap();
    assert!(title.actionable);
    let point = (title.rect.x, title.rect.y);
    for event in [
        InputEvent::mouse_down(PointerButton::Left, point),
        InputEvent::mouse_up(PointerButton::Left, point),
    ] {
        assert!(!workspace.input(&event, &mut state).unwrap());
        state.apply_input(event);
    }
    assert_eq!(state.active_screen(), ShellScreen::ExitConfirm);
    draw(&mut state, &mut workspace);
    assert_eq!(workspace.split_window, Some(id));
    let escape = InputEvent::key(InputKey::Escape);
    assert!(!workspace.input(&escape, &mut state).unwrap());
    state.apply_input(escape);
    let snapshot = draw(&mut state, &mut workspace);
    assert_eq!(state.active_screen(), ShellScreen::Home);
    assert_eq!(workspace.split_window, Some(id));
    assert!(
        snapshot
            .nodes
            .iter()
            .any(|node| node.id == "chrome/shell.back" && node.actionable)
    );
}

fn render(
    workspace: &mut Workspace,
    terminal: &mut Terminal<TestBackend>,
    context: &ui::RenderContext,
    picker: Option<&ui::EditorImagePicker>,
    caches: &mut HashMap<SessionId, PaneImages>,
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
    wrong_size.cells = Arc::new(vec![
        workspace_protocol::Cell::default();
        wrong_size.size.area()
    ]);
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
fn native_image_encoding_keeps_cells_visible_and_discards_a_superseded_placement() {
    use watchdog::{WatchdogConfig, WatchdogRuntime};
    let dir = tempfile::tempdir().unwrap();
    let (runtime, process) = WatchdogRuntime::start_isolated(WatchdogConfig::new(
        dir.path().join("reports"),
        dir.path().join("fallback"),
        dir.path().join("data"),
        "workspace-images",
        "test",
    ))
    .unwrap();
    let app = process
        .register_app(watchdog::AppDescriptor::new(
            watchdog::AppId::from_static("shell"),
            "Shell",
            "test",
            watchdog::AppCriticality::Optional,
        ))
        .unwrap();
    let group = app.task_group("images");
    let probe = ui::TerminalGraphicsProbe::from_terminal_capabilities(
        ui::EditorGraphicsProtocol::Iterm2,
        2,
        2,
        false,
        false,
    );
    let mut terminal = Terminal::new(TestBackend::new(8, 2)).unwrap();
    let mut pane = PaneImages::default();
    let image_at = |x| {
        let area = workspace_protocol::Rect {
            x,
            y: 0,
            width: 1,
            height: 1,
        };
        workspace_protocol::Image {
            image_id: u64::from(x),
            preview_id: None,
            width: 2,
            height: 2,
            rgba: Arc::from([255_u8; 16]),
            source: [0, 0, 2, 2],
            padding: [0; 4],
            cell_size: [2, 2],
            area,
            visible: vec![area],
            z_index: 0,
        }
    };
    let previous = image_at(1);
    let current = image_at(4);
    let mut present = |pane: &mut PaneImages, image: &workspace_protocol::Image| {
        terminal
            .draw(|frame| {
                frame.buffer_mut()[(1, 0)].set_symbol("A");
                frame.buffer_mut()[(4, 0)].set_symbol("B");
                pane.render(
                    frame,
                    frame.area(),
                    std::slice::from_ref(image),
                    probe.picker().unwrap(),
                    ratatui::style::Color::Black,
                    Some(&group),
                );
            })
            .unwrap();
        [1, 4].map(|x| terminal.backend().buffer()[(x, 0)].symbol().to_string())
    };
    let wait = |pane: &PaneImages| {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !pane.ready() {
            assert!(Instant::now() < deadline, "native image was not prepared");
            std::thread::sleep(Duration::from_millis(1));
        }
    };
    assert_eq!(present(&mut pane, &previous), ["A", "B"]);
    wait(&pane);
    assert_eq!(present(&mut pane, &current), ["A", "B"]);
    wait(&pane);
    let cells = present(&mut pane, &current);
    assert_eq!(cells[0], "A");
    assert!(cells[1].contains("\x1b]1337;File="));
    drop(pane);
    runtime.shutdown().unwrap();
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
        preview_id: None,
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
