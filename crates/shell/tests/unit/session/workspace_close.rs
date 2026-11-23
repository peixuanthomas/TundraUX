use super::*;

pub(super) fn window(minimum: Size) -> Window {
    Window {
        launch: WindowLaunch::Clock,
        worker: None,
        stream: None,
        init: None,
        frame: None,
        presented: None,
        presenting: None,
        starting_presenting: false,
        status: WindowStatus {
            minimum,
            ..WindowStatus::default()
        },
        state: ProcessState::Running,
        queued: VecDeque::new(),
        viewport: None,
        close: CloseProgress::Idle,
        picked_file: None,
        force_when_started: false,
        view_requested: None,
        launch_error: false,
        input_blocked: false,
        startup: None,
    }
}

pub(super) fn connected_window(
    minimum: Size,
) -> (
    Window,
    platform::shell_control::AsyncLocalConnection,
    platform::shell_control::AsyncLocalListener,
) {
    let name = format!(
        "tundra-prefix-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    #[cfg(unix)]
    let endpoint = std::env::temp_dir()
        .join(format!("{name}.sock"))
        .to_str()
        .unwrap()
        .to_owned();
    #[cfg(windows)]
    let endpoint = name;
    let listener = {
        let _runtime = crate::process_host::event_loop::runtime().enter();
        platform::shell_control::AsyncLocalListener::bind(&endpoint).unwrap()
    };
    let (connection, peer) = crate::process_host::event_loop::runtime().block_on(async {
        let connection =
            platform::shell_control::AsyncLocalConnection::connect(&endpoint, std::process::id())
                .await
                .unwrap();
        let peer = listener.accept().await.unwrap();
        (connection, peer)
    });
    let mut app = window(minimum);
    app.stream = Some(WindowConnection::new(
        connection,
        workspace_protocol::MAX_WINDOW_FRAME_BYTES,
    ));
    (app, peer, listener)
}

#[test]
#[cfg_attr(not(feature = "native-tests"), ignore = "requires local socket access")]
fn welcome_sends_latest_initialization_without_another_workspace_poll() {
    let mut state = shell_state();
    let mut workspace = Workspace::new(None, false);
    let id = SessionId(1);
    let worker = WorkerId {
        run: workspace_protocol::RunId(1),
        role: WorkerRole::App(id),
        generation: workspace_protocol::ProcessGeneration(3),
    };
    let (mut app, peer, _listener) = connected_window(Size::new(106, 16));
    app.worker = Some(worker);
    app.init = Some(WindowInit {
        launch: serde_json::Value::Null,
        profile: None,
        language: None,
        size: Size::new(106, 16),
        layout: LayoutGeneration(1),
        focused: false,
        image_cell_size: None,
        text_sizing: false,
        ansi_colors: false,
    });
    let viewport = ShellToApp::Viewport {
        layout: LayoutGeneration(9),
        size: Some(Size::new(118, 32)),
        visible: true,
        focused: true,
    };
    app.viewport = Some(viewport.clone());
    let catalog = ShellToApp::TaskCatalog {
        ids: vec!["existing-task".into()],
    };
    app.queued.push_back(catalog.clone());
    workspace.windows.insert(id, app);
    workspace
        .receive(id, AppToShell::Welcome { worker }, &mut state)
        .unwrap();
    let mut peer = peer.into_inner();
    let messages = crate::process_host::event_loop::runtime().block_on(async {
        tokio::time::timeout(Duration::from_secs(2), async {
            let mut messages = Vec::new();
            for _ in 0..3 {
                let bytes = workspace_protocol::transport::read_payload(
                    &mut peer,
                    workspace_protocol::MAX_WINDOW_FRAME_BYTES,
                )
                .await
                .unwrap();
                messages.push(serde_json::from_slice::<ShellToApp>(&bytes).unwrap());
            }
            messages
        })
        .await
        .unwrap()
    });
    let ShellToApp::Init(init) = &messages[0] else {
        panic!("initialization must precede viewport and controls");
    };
    assert_eq!(init.layout, LayoutGeneration(9));
    assert_eq!(init.size, Size::new(118, 32));
    assert!(init.focused);
    assert!(init.language.is_some());
    assert_eq!(messages[1], viewport);
    assert_eq!(messages[2], catalog);
}

fn show(workspace: &mut Workspace, id: SessionId) {
    if let Some(source) = workspace.selection.focused() {
        workspace
            .selection
            .split(source, WorkspaceSplitMode::LeftRight);
    }
    workspace.selection.open(id);
}

#[test]
fn only_a_completed_picker_returns_a_selected_path_to_its_opener() {
    for (forced, cancelled) in [(false, false), (true, false), (false, true)] {
        let mut state = shell_state();
        let mut workspace = Workspace::new(None, false);
        let (opener, picker) = (SessionId(1), SessionId(2));
        let path = NativePath(PathBuf::from("日志.txt"));
        workspace.windows.insert(opener, window(Size::new(106, 16)));
        let mut picker_window = window(Size::new(106, 16));
        picker_window.launch = WindowLaunch::ExplorerPicker {
            request: 7,
            suggested_name: None,
        };
        workspace.windows.insert(picker, picker_window);
        workspace.selection.open(opener);
        workspace
            .selection
            .open_from(opener, picker, &workspace.layout);

        workspace
            .receive(picker, AppToShell::FilePicked(path.clone()), &mut state)
            .unwrap();
        assert_eq!(
            workspace.windows[&picker].queued.back(),
            Some(&ShellToApp::RequestClose)
        );
        assert!(workspace.windows[&opener].queued.is_empty());
        if cancelled {
            workspace
                .receive(picker, AppToShell::CloseCancelled, &mut state)
                .unwrap();
        }
        workspace.finish_window_exit(picker, Some(0), forced, &mut state);
        assert_eq!(
            workspace.windows[&opener].queued.back(),
            Some(&ShellToApp::FilePickerResult {
                request: 7,
                path: (!forced && !cancelled).then_some(path)
            })
        );
        assert_eq!(workspace.selection.focused(), Some(opener));
    }
}

pub(super) fn shell_state() -> ShellSession {
    ShellSession::new_for_home_mode(ShellLaunchConfig::default(), (108, 20), ShellHomeMode::User)
}

#[test]
fn only_the_explicit_debug_message_reaches_the_shell_panic_boundary() {
    use watchdog::{BoundaryKind, BoundarySpec, WatchdogConfig, WatchdogRuntime};
    let root = std::env::temp_dir().join(format!(
        "tundra-workspace-debug-{}-{}",
        std::process::id(),
        PageInstanceId::new().get()
    ));
    let (runtime, process) = WatchdogRuntime::start_isolated(WatchdogConfig::new(
        root.join("reports"),
        root.join("fallback"),
        root.join("data"),
        "shell-debug-route",
        "test",
    ))
    .unwrap();
    let app = process
        .register_app(watchdog::AppDescriptor::new(
            watchdog::AppId::from_static("shell"),
            "Shell",
            "test",
            watchdog::AppCriticality::ProcessCritical,
        ))
        .unwrap();
    let mut state = shell_state();
    let mut workspace = Workspace::new(None, false);
    let id = SessionId(1);
    workspace.windows.insert(id, window(Size::new(106, 16)));
    let caught = app
        .run_boundary(
            BoundarySpec::new("shell.fullscreen-session", BoundaryKind::UiSession),
            std::panic::AssertUnwindSafe(|| {
                workspace
                    .receive(
                        id,
                        AppToShell::ShellAction(HostAction::DebugPanicRequested),
                        &mut state,
                    )
                    .unwrap()
            }),
        )
        .expect_err("the explicit request must unwind the Shell boundary");
    let incident = caught.incident_id().to_string();
    let message = super::super::runtime::finalize_session_panic(caught, "Shell UI");
    assert!(message.contains("Intentional watchdog panic test requested from Command Line"));
    assert!(message.contains("Full report:"));
    let report = process
        .list_incident_reports()
        .reports
        .into_iter()
        .find(|report| report.incident_id == incident)
        .unwrap();
    assert_eq!(report.boundary, "shell.fullscreen-session");
    assert_eq!(report.kind, watchdog::IncidentKind::Panic);
    assert!(matches!(
        report.recovery,
        watchdog::RecoveryOutcome::Unrecoverable(_)
    ));
    drop(app);
    drop(process);
    runtime.shutdown().unwrap();
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn ordinary_app_exits_including_76_keep_other_windows_alive() {
    for code in [Some(76), Some(101), None] {
        let mut state = shell_state();
        let mut workspace = Workspace::new(None, false);
        let (failed, remaining) = (SessionId(1), SessionId(2));
        for id in [failed, remaining] {
            workspace.windows.insert(id, window(Size::new(106, 16)));
            show(&mut workspace, id);
        }
        workspace.finish_window_exit(failed, code, false, &mut state);
        assert!(!workspace.windows.contains_key(&failed));
        assert!(workspace.windows.contains_key(&remaining));
        assert_eq!(workspace.selection.focused(), Some(remaining));
        assert!(!workspace.shutdown_requested());
        assert!(workspace.pending_global.is_none());
    }
}

fn pointer(
    workspace: &mut Workspace,
    state: &mut ShellSession,
    (x, y): CellPosition,
    kind: ui::MouseEventKind,
) -> bool {
    workspace
        .input(&InputEvent::Mouse(MouseInput::new(x, y, kind)), state)
        .unwrap()
}

#[test]
fn minimized_window_and_status_details_use_separate_click_targets() {
    use crate::session::compositor::ScreenCompositor;
    use ratatui::{Terminal, backend::TestBackend};

    let mut state = shell_state();
    state.set_navigation_path(vec![ShellScreen::Home]);
    state.notify_status("Workspace status");
    let mut workspace = Workspace::new(None, false);
    let id = SessionId(1);
    workspace.windows.insert(id, window(Size::new(106, 16)));
    workspace.taskbar_order.push(id);
    show(&mut workspace, id);
    workspace.selection.minimize(id);

    let mut terminal = Terminal::new(TestBackend::new(108, 20)).unwrap();
    let mut compositor = ScreenCompositor::default();
    let host = CommandLineHost::new(default_editor_watchdog().unwrap());
    let prepared = compositor.prepare(
        &state,
        &host,
        Instant::now(),
        Default::default(),
        ui::RenderContext::default(),
        None,
    );
    terminal
        .draw(|frame| {
            compositor.render(frame, &mut state, &prepared, None, Some(&mut workspace));
        })
        .unwrap();

    let status = state.frame_layout.unwrap().status_message.unwrap();
    assert_eq!(status.y, 0);
    let point = (status.x, status.y);
    let down = InputEvent::mouse_down(PointerButton::Left, point);
    let up = InputEvent::mouse_up(PointerButton::Left, point);
    assert!(!workspace.input(&down, &mut state).unwrap());
    state.apply_input(down);
    assert!(!state.status_details_visible());
    assert!(!workspace.input(&up, &mut state).unwrap());
    state.apply_input(up);
    assert!(state.status_details_visible());
    assert_eq!(
        state.to_notification_view_model().unwrap().message,
        "Workspace status"
    );
    state.notification_dismiss_active_modal_without_response();

    let taskbar = workspace.taskbar.as_ref().unwrap();
    assert_eq!(taskbar.area, Rect::new(0, 19, 108, 1));
    let (_, entry) = taskbar
        .hits
        .iter()
        .find(|(action, _)| *action == WorkspaceTaskbarAction::Activate(id))
        .unwrap();
    let point = (entry.x, entry.y);
    assert!(pointer(
        &mut workspace,
        &mut state,
        point,
        ui::MouseEventKind::Down(PointerButton::Left)
    ));
    assert!(pointer(
        &mut workspace,
        &mut state,
        point,
        ui::MouseEventKind::Up(PointerButton::Left)
    ));
    assert_eq!(workspace.selection.focused(), Some(id));
}

#[test]
fn closing_window_keeps_reported_minimum_until_guardian_removal() {
    let id = SessionId(1);
    let minimum = Size::new(130, 25);
    let mut window = window(minimum);
    window.close = CloseProgress::Cleaning(Instant::now());
    let bounds = Rect::new(0, 0, 120, 25);
    let content = Rect::new(0, 1, 120, 23);
    window.finish_cleanup();
    window.update_status(WindowStatus::default());
    assert_eq!(window.close, CloseProgress::Exiting);
    assert_eq!(window.status.minimum, minimum);
    assert_eq!(
        WorkspaceLayout::capacity(bounds, content, &[(id, window.status.minimum)]),
        0,
    );
    assert_eq!(WorkspaceLayout::capacity(bounds, content, &[]), 1,);
}

#[test]
fn displayed_close_wait_consumes_its_deadline_without_masking_starting_frames() {
    let expired = Instant::now() - CLOSE_WAIT - Duration::from_secs(1);
    for close in [
        CloseProgress::Requested(expired),
        CloseProgress::Cleaning(expired),
    ] {
        let id = SessionId(1);
        let mut workspace = Workspace::new(None, false);
        let mut app = window(Size::new(106, 16));
        app.close = close;
        workspace.windows.insert(id, app);
        assert_eq!(workspace.poll_timeout(), Duration::ZERO);

        workspace.show_close_wait(id);
        assert_eq!(workspace.poll_timeout(), watchdog::WORKER_PROGRESS_INTERVAL);
        assert_eq!(workspace.windows[&id].close, close);

        workspace.placeholder_redraw_at = Some(Instant::now());
        assert_eq!(workspace.poll_timeout(), Duration::ZERO);
    }
}

#[test]
fn exited_window_frame_and_taskbar_hits_do_not_survive_until_redraw() {
    let mut state = shell_state();
    let (first, second) = (SessionId(1), SessionId(2));
    let mut workspace = Workspace::new(None, false);
    for id in [first, second] {
        workspace
            .windows
            .insert(id, window(WindowStatus::default().minimum));
        show(&mut workspace, id);
    }
    let bounds = Rect::new(0, 0, 240, 20);
    workspace.relayout(bounds, Rect::new(0, 1, 240, 18), false);
    let close = workspace.layout.panes[0].controls.close;
    let taskbar = WorkspaceTaskbar::new(
        Rect::new(0, 19, 200, 1),
        &[WorkspaceTaskbarItem {
            id: first,
            title: "Clock".into(),
            focused: false,
            minimized: false,
            closing: false,
            unresponsive: false,
        }],
        0,
    );
    let first_tab = taskbar.hits[0].1;
    workspace.taskbar = Some(taskbar);
    let down = ui::MouseEventKind::Down(PointerButton::Left);
    let up = ui::MouseEventKind::Up(PointerButton::Left);

    assert!(pointer(
        &mut workspace,
        &mut state,
        (close.x, close.y),
        down
    ));
    assert_eq!(workspace.windows[&first].close, CloseProgress::Idle);
    workspace.remove(first);
    assert!(pointer(&mut workspace, &mut state, (close.x, close.y), up));
    for area in [close, first_tab] {
        pointer(&mut workspace, &mut state, (area.x, area.y), down);
        pointer(&mut workspace, &mut state, (area.x, area.y), up);
    }
    assert!(!workspace.windows.contains_key(&first));
    assert!(!workspace.selection.includes(first));
    assert_eq!(workspace.selection.focused(), Some(second));
    assert_eq!(workspace.windows[&second].close, CloseProgress::Idle);
}

#[test]
fn taskbar_cross_closes_its_window_on_release() {
    let mut workspace = Workspace::new(None, false);
    let mut state = shell_state();
    let foreground = SessionId(1);
    let target = SessionId(2);
    for id in [target, foreground] {
        workspace.windows.insert(id, window(Size::new(106, 16)));
        workspace.selection.open(id);
    }
    let taskbar = WorkspaceTaskbar::new(
        Rect::new(0, 19, 108, 1),
        &[WorkspaceTaskbarItem {
            id: target,
            title: "Clock".into(),
            focused: false,
            minimized: true,
            closing: false,
            unresponsive: false,
        }],
        0,
    );
    workspace.taskbar = Some(taskbar);
    let down = ui::MouseEventKind::Down(PointerButton::Left);
    let up = ui::MouseEventKind::Up(PointerButton::Left);
    pointer(&mut workspace, &mut state, (1, 19), down);
    pointer(&mut workspace, &mut state, (4, 19), up);
    assert_eq!(workspace.windows[&target].close, CloseProgress::Idle);
    assert_eq!(workspace.selection.focused(), Some(foreground));

    pointer(&mut workspace, &mut state, (1, 19), down);
    assert_eq!(workspace.windows[&target].close, CloseProgress::Idle);
    pointer(&mut workspace, &mut state, (1, 19), up);
    assert!(matches!(
        workspace.windows[&target].close,
        CloseProgress::Requested(_)
    ));
    assert_eq!(workspace.windows[&foreground].close, CloseProgress::Idle);
    assert_eq!(
        workspace.windows[&target].queued.back(),
        Some(&ShellToApp::RequestClose)
    );
}

#[test]
fn ordinary_open_reuses_the_exact_application_and_preserves_split_add() {
    let mut workspace = Workspace::new(None, false);
    let editor = SessionId(1);
    let script = SessionId(2);
    for (id, launch) in [
        (editor, WindowLaunch::EditorPath("/draft.txt".into())),
        (script, WindowLaunch::Script),
    ] {
        let mut app = window(Size::new(106, 16));
        app.launch = launch;
        workspace.windows.insert(id, app);
        workspace.selection.open(id);
    }
    workspace
        .selection
        .split(script, WorkspaceSplitMode::LeftRight);
    workspace
        .open_requested(WindowLaunch::Editor, false, None, None)
        .unwrap();
    assert_eq!(workspace.selection.visible(), vec![script, editor]);
    assert_eq!(workspace.selection.focused(), Some(editor));
    assert_eq!(workspace.windows.len(), 2);

    let original = &workspace.windows[&editor].launch;
    assert!(!WindowLaunch::EditorPath("/other.txt".into()).can_reuse(original));
    assert!(!WindowLaunch::Editor.can_reuse(&WindowLaunch::Script));
    assert_eq!(original.for_new_session(), WindowLaunch::Editor);
}

#[test]
fn taskbar_context_moves_only_the_taskbar_order() {
    let mut workspace = Workspace::new(None, false);
    let mut state = shell_state();
    let (first, second) = (SessionId(1), SessionId(2));
    for id in [first, second] {
        workspace.windows.insert(id, window(Size::new(106, 16)));
        workspace.taskbar_order.push(id);
        show(&mut workspace, id);
    }
    workspace.relayout(Rect::new(0, 0, 220, 20), Rect::new(0, 1, 220, 18), false);
    let area = Rect::new(0, 19, 110, 1);
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(220, 20)).unwrap();
    terminal
        .draw(|frame| {
            workspace.render_taskbar(
                frame,
                Some(area),
                &ui::RenderContext::default(),
                None,
                false,
            )
        })
        .unwrap();
    let taskbar = workspace.taskbar.as_ref().unwrap();
    let (_, title) = taskbar
        .hits
        .iter()
        .find(|(action, _)| *action == WorkspaceTaskbarAction::Activate(first))
        .unwrap();
    let point = (title.x, title.y);
    pointer(
        &mut workspace,
        &mut state,
        point,
        ui::MouseEventKind::Down(PointerButton::Right),
    );
    pointer(
        &mut workspace,
        &mut state,
        point,
        ui::MouseEventKind::Up(PointerButton::Right),
    );
    let menu = workspace.taskbar_menu.as_ref().unwrap();
    assert_eq!(
        menu.menu
            .items
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        ["open", "new-session", "move"]
    );
    for key in [
        InputKey::Down,
        InputKey::Down,
        InputKey::Enter,
        InputKey::Enter,
    ] {
        workspace.input(&InputEvent::key(key), &mut state).unwrap();
    }
    assert!(workspace.taskbar_menu.is_none());
    assert_eq!(workspace.taskbar_order, vec![second, first]);
    assert_eq!(workspace.selection.visible(), vec![first, second]);
    assert_eq!(workspace.selection.focused(), Some(second));
}

#[test]
fn stale_window_list_keyboard_focus_does_not_restore_an_exited_window() {
    let id = SessionId(1);
    let mut workspace = Workspace::new(None, false);
    workspace
        .windows
        .insert(id, window(WindowStatus::default().minimum));
    show(&mut workspace, id);
    workspace.panel = Some(WorkspacePanelViewModel {
        selected: Some(id),
        action_focus: Some(Action::Switch(id)),
        ..Default::default()
    });
    workspace
        .panel_hits
        .push((Action::Switch(id), Rect::new(0, 0, 10, 1)));

    workspace.remove(id);
    workspace
        .input(
            &InputEvent::Key(KeyInput::new(InputKey::Enter)),
            &mut shell_state(),
        )
        .unwrap();
    assert_eq!(workspace.selection.focused(), None);
    assert!(!workspace.selection.includes(id));
}

#[test]
fn unmatched_window_search_cannot_activate_a_stale_selection_before_redraw() {
    let mut workspace = Workspace::new(None, false);
    let (background, active) = (SessionId(1), SessionId(2));
    for id in [background, active] {
        workspace.windows.insert(id, window(Size::new(106, 16)));
        workspace.selection.open(id);
    }
    workspace.panel = Some(WorkspacePanelViewModel {
        selected: Some(background),
        ..Default::default()
    });
    let mut state = shell_state();
    workspace
        .input(&InputEvent::Paste("no matching window".into()), &mut state)
        .unwrap();
    workspace
        .input(&InputEvent::key(InputKey::Enter), &mut state)
        .unwrap();
    assert_eq!(workspace.selection.focused(), Some(active));
    assert!(workspace.panel.is_some());
}

#[test]
fn a_blocked_window_keeps_close_controls_and_unfreezes_its_confirmation_on_resize() {
    let mut state = shell_state();
    let id = SessionId(1);
    let mut workspace = Workspace::new(None, false);
    workspace.windows.insert(id, window(Size::new(130, 25)));
    show(&mut workspace, id);
    let small = Rect::new(0, 0, 108, 20);
    let small_content = Rect::new(0, 1, 108, 18);
    workspace.relayout(small, small_content, false);
    let close = workspace.blocked_pane().unwrap().controls.close;
    let down = ui::MouseEventKind::Down(PointerButton::Left);
    let up = ui::MouseEventKind::Up(PointerButton::Left);
    assert!(pointer(
        &mut workspace,
        &mut state,
        (close.x, close.y),
        down
    ));
    assert!(pointer(&mut workspace, &mut state, (close.x, close.y), up));
    let window = workspace.windows.get_mut(&id).unwrap();
    assert!(matches!(window.close, CloseProgress::Requested(_)));
    assert!(matches!(
        window.viewport,
        Some(ShellToApp::Viewport {
            size: None,
            visible: false,
            ..
        })
    ));
    window.close = CloseProgress::Confirmation {
        size_notice_dismissed: false,
    };

    workspace.relayout(small, small_content, false);
    assert_eq!(workspace.wait_for, Some(id));
    assert_eq!(workspace.input_focus, None);
    workspace
        .panel_hits
        .push((Action::Dismiss, Rect::new(0, 0, 10, 1)));

    workspace.relayout(Rect::new(0, 0, 132, 29), Rect::new(0, 1, 132, 27), false);
    assert_eq!(workspace.wait_for, None);
    assert!(workspace.panel_hits.is_empty());
    assert_eq!(workspace.input_focus, Some(id));
    assert!(matches!(
        workspace.windows[&id].viewport,
        Some(ShellToApp::Viewport {
            size: Some(Size {
                width: 130,
                height: 25
            }),
            visible: true,
            focused: true,
            ..
        })
    ));
    assert!(matches!(
        workspace.windows[&id].close,
        CloseProgress::Confirmation { .. }
    ));
}

#[test]
fn split_chooser_cancellation_and_resize_restore_only_the_pending_region() {
    let mut state = shell_state();
    let id = SessionId(1);
    let mut workspace = Workspace::new(None, false);
    workspace.windows.insert(id, window(Size::new(106, 16)));
    workspace.selection.open(id);
    let bounds = Rect::new(0, 0, 217, 39);
    workspace.relayout(bounds, Rect::new(0, 1, 217, 37), false);
    let previous = workspace.selection.tree.clone();
    workspace.begin_split(id, WorkspaceSplitMode::LeftRight, &state);
    assert!(workspace.chooser.is_some());
    assert_eq!(workspace.layout.panes[0].id, id);
    assert_eq!(workspace.layout.panes[1].id, SessionId(0));
    assert!(!workspace.animating());
    assert!(!workspace.external_snapshot_ready());
    let mut frame = WindowFrame::new(workspace.generation, Size::new(106, 35));
    frame.animation_running = true;
    workspace.windows.get_mut(&id).unwrap().frame = Some(Arc::new(frame));
    assert!(workspace.animating());
    assert!(workspace.external_snapshot_ready());
    workspace
        .input(&InputEvent::key(InputKey::Escape), &mut state)
        .unwrap();
    assert_eq!(workspace.selection.tree, previous);
    assert_eq!(workspace.selection.focused(), Some(id));
    workspace.relayout(bounds, Rect::new(0, 1, 217, 37), false);
    workspace.begin_split(id, WorkspaceSplitMode::TopBottom, &state);
    workspace.relayout(Rect::new(0, 0, 108, 20), Rect::new(0, 1, 108, 18), false);
    assert!(workspace.chooser.is_none());
    assert_eq!(workspace.selection.tree, previous);
}

#[test]
fn pointer_input_after_split_selection_ignores_the_old_chooser_area() {
    use ratatui::{Terminal, backend::TestBackend};

    let mut state = shell_state();
    while state.notification_dismiss_active_modal_without_response() {}
    let mut workspace = Workspace::new(None, false);
    let (source, restored) = (SessionId(1), SessionId(2));
    for id in [source, restored] {
        workspace.windows.insert(id, window(Size::new(106, 16)));
    }
    workspace.selection.open(source);
    let bounds = Rect::new(0, 0, 240, 36);
    workspace.relayout(bounds, Rect::new(0, 1, 240, 34), false);
    workspace.begin_split(source, WorkspaceSplitMode::LeftRight, &state);

    let mut terminal = Terminal::new(TestBackend::new(bounds.width, bounds.height)).unwrap();
    let mut snapshot = None;
    terminal
        .draw(|frame| {
            let capture = ui::automation::FrameCapture::begin(bounds, "chooser");
            workspace.render_chooser(frame, &ui::RenderContext::default(), Some(&capture));
            snapshot = Some(capture.finish(frame.buffer_mut()));
        })
        .unwrap();
    let snapshot = snapshot.unwrap();
    let row = snapshot
        .nodes
        .iter()
        .find(|node| node.id.ends_with("window.2"))
        .unwrap()
        .rect;
    for kind in [
        ui::MouseEventKind::Down(PointerButton::Left),
        ui::MouseEventKind::Up(PointerButton::Left),
    ] {
        assert!(pointer(&mut workspace, &mut state, (row.x, row.y), kind));
    }
    assert!(workspace.chooser.is_none());
    assert_eq!(workspace.selection.focused(), Some(restored));
    assert!(
        workspace
            .layout
            .panes
            .iter()
            .any(|pane| pane.id == SessionId(0))
    );

    for kind in [
        ui::MouseEventKind::Moved,
        ui::MouseEventKind::Down(PointerButton::Left),
        ui::MouseEventKind::Up(PointerButton::Left),
    ] {
        assert!(pointer(&mut workspace, &mut state, (row.x, row.y), kind));
        assert!(workspace.capture.is_none());
    }
    assert!(!workspace.take_input_rejection().0);
    assert_eq!(workspace.selection.visible(), vec![source, restored]);
    assert_eq!(workspace.windows[&restored].close, CloseProgress::Idle);
}

#[cfg(unix)]
#[test]
#[cfg_attr(not(feature = "native-tests"), ignore = "requires local socket access")]
fn frozen_native_terminal_keeps_f1_and_ctrl_w_without_arming_the_prefix() {
    let mut state = shell_state();
    let id = SessionId(1);
    let mut workspace = Workspace::new(None, false);
    let (app, peer, listener) = connected_window(Size::new(106, 16));
    workspace.windows.insert(id, app);
    workspace.selection.open(id);
    workspace.relayout(Rect::new(0, 0, 217, 39), Rect::new(0, 1, 217, 37), false);
    let mut frame = WindowFrame::new(
        LayoutGeneration(workspace.generation.0.saturating_sub(1)),
        Size::new(215, 35),
    );
    frame.ctrl_w_passthrough = true;
    frame.native_escape.push(workspace_protocol::Rect {
        x: 0,
        y: 0,
        width: 215,
        height: 35,
    });
    workspace.windows.get_mut(&id).unwrap().frame = Some(Arc::new(frame));
    assert!(workspace.has_native_input());
    workspace
        .input(&InputEvent::key(InputKey::F(1)), &mut state)
        .unwrap();
    workspace
        .input(
            &InputEvent::key_with_modifiers(InputKey::Char('w'), crate::InputModifiers::CTRL),
            &mut state,
        )
        .unwrap();
    workspace
        .input(&InputEvent::from_key_label("v"), &mut state)
        .unwrap();
    assert!(workspace.chooser.is_none());
    assert_eq!(workspace.selection.visible(), vec![id]);
    assert_eq!(
        workspace.windows[&id]
            .queued
            .iter()
            .filter(|message| matches!(message, ShellToApp::Input { .. }))
            .count(),
        3
    );
    assert!(
        !workspace.windows[&id]
            .queued
            .iter()
            .any(|message| matches!(message, ShellToApp::Help { .. }))
    );
    drop(workspace);
    drop(peer);
    drop(listener);
}

#[cfg(unix)]
#[test]
#[cfg_attr(not(feature = "native-tests"), ignore = "requires local socket access")]
fn saturated_window_cancels_pointer_capture_and_keeps_global_shortcuts() {
    use crate::process_host::transport::{MAX_MESSAGES, RESERVED_MESSAGES};
    let mut state = shell_state();
    let id = SessionId(1);
    let mut workspace = Workspace::new(None, false);
    let (app, _peer, _listener) = connected_window(Size::new(106, 16));
    workspace.windows.insert(id, app);
    workspace.selection.open(id);
    workspace.relayout(Rect::new(0, 0, 217, 39), Rect::new(0, 1, 217, 37), false);
    let content = workspace.layout.panes[0].content;
    let mut frame = WindowFrame::new(
        workspace.generation,
        Size::new(content.width, content.height),
    );
    frame.sequence = 1;
    frame.ctrl_w_passthrough = true;
    workspace
        .receive(id, AppToShell::Frame(frame), &mut state)
        .unwrap();
    workspace.capture = Some(PointerCapture {
        target: PointerTarget::Pane(id),
        generation: workspace.generation,
        button: PointerButton::Left,
    });
    let mut accepted = 0;
    while workspace.enqueue(
        id,
        ShellToApp::Input {
            serial: 0,
            layout: workspace.generation,
            event: serde_json::Value::Null,
        },
    ) {
        accepted += 1;
        assert!(accepted <= MAX_MESSAGES - RESERVED_MESSAGES);
    }
    assert!(workspace.windows[&id].input_blocked);
    assert!(workspace.capture.is_none());
    assert!(matches!(
        workspace.windows[&id].queued.back(),
        Some(ShellToApp::CancelPointer { .. })
    ));
    assert_eq!(workspace.take_input_rejection(), (true, true));
    workspace
        .input(
            &InputEvent::key_with_modifiers(InputKey::Char('w'), crate::InputModifiers::CTRL),
            &mut state,
        )
        .unwrap();
    workspace
        .input(&InputEvent::from_key_label("v"), &mut state)
        .unwrap();
    assert!(workspace.chooser.is_some());
    workspace.request_close(id);
    assert!(matches!(
        workspace.windows[&id].queued.back(),
        Some(ShellToApp::RequestClose)
    ));
    assert!(workspace.windows[&id].queued.len() <= MAX_MESSAGES);
}

#[test]
fn visible_input_fence_requires_a_current_frame_and_terminal_states_release_it() {
    let mut state = shell_state();
    let id = SessionId(1);
    let mut workspace = Workspace::new(None, false);
    workspace.windows.insert(id, window(Size::new(106, 16)));
    workspace.selection.open(id);
    workspace.relayout(Rect::new(0, 0, 108, 20), Rect::new(0, 1, 108, 18), false);
    workspace.input_fences.insert(id, 42);
    workspace
        .receive(id, AppToShell::InputFence { serial: 42 }, &mut state)
        .unwrap();
    assert_eq!(workspace.input_fences.get(&id), Some(&42));
    let content = workspace.layout.panes[0].content;
    let mut frame = WindowFrame::new(
        workspace.generation,
        Size::new(content.width, content.height),
    );
    frame.sequence = 1;
    frame.processed_input_serial = 99;
    frame.confirmed_input_fence = 41;
    workspace
        .receive(id, AppToShell::Frame(frame.clone()), &mut state)
        .unwrap();
    assert_eq!(workspace.input_fences.get(&id), Some(&42));
    frame.sequence = 2;
    frame.confirmed_input_fence = 42;
    workspace
        .receive(id, AppToShell::Frame(frame.clone()), &mut state)
        .unwrap();
    assert!(!workspace.input_fences.contains_key(&id));
    workspace.input_fences.insert(id, 43);
    frame.sequence = 1;
    frame.confirmed_input_fence = 99;
    workspace
        .receive(id, AppToShell::Frame(frame), &mut state)
        .unwrap();
    assert_eq!(workspace.input_fences.get(&id), Some(&43));
    workspace
        .receive(id, AppToShell::CloseStarted, &mut state)
        .unwrap();
    assert!(!workspace.input_fences.contains_key(&id));
}

#[test]
fn background_exit_during_selection_does_not_cancel_the_pending_split() {
    let state = shell_state();
    let (active, background) = (SessionId(1), SessionId(2));
    let mut workspace = Workspace::new(None, false);
    for id in [background, active] {
        workspace.windows.insert(id, window(Size::new(106, 16)));
        workspace.selection.open(id);
    }
    workspace.relayout(Rect::new(0, 0, 217, 39), Rect::new(0, 1, 217, 37), false);
    workspace.begin_split(active, WorkspaceSplitMode::LeftRight, &state);
    workspace.remove(background);
    assert!(workspace.chooser.is_some());
    assert!(workspace.selection.visible().contains(&SessionId(0)));
    workspace.remove(active);
    assert!(workspace.chooser.is_none());
    assert!(workspace.selection.visible().is_empty());
}

#[test]
fn directional_focus_does_not_jump_diagonally_across_a_t_layout() {
    let (left, top, bottom) = (SessionId(1), SessionId(2), SessionId(3));
    let mut workspace = Workspace::new(None, false);
    let mut state = shell_state();
    let navigate = |workspace: &mut Workspace, state: &mut ShellSession, direction: char| {
        workspace
            .input(
                &InputEvent::key_with_modifiers(InputKey::Char('w'), crate::InputModifiers::CTRL),
                state,
            )
            .unwrap();
        workspace
            .input(&InputEvent::key(InputKey::Char(direction)), state)
            .unwrap();
    };
    for id in [left, top, bottom] {
        workspace.windows.insert(id, window(Size::new(106, 16)));
    }
    workspace.selection.open(left);
    workspace
        .selection
        .split(left, WorkspaceSplitMode::LeftRight);
    workspace.selection.open(top);
    workspace
        .selection
        .split(top, WorkspaceSplitMode::TopBottom);
    workspace.selection.open(bottom);
    workspace.selection.focus(left);
    workspace.relayout(Rect::new(0, 0, 217, 39), Rect::new(0, 1, 217, 37), false);
    navigate(&mut workspace, &mut state, 'j');
    assert_eq!(workspace.selection.focused(), Some(left));
    navigate(&mut workspace, &mut state, 'l');
    assert_eq!(workspace.selection.focused(), Some(top));
    navigate(&mut workspace, &mut state, 'j');
    assert_eq!(workspace.selection.focused(), Some(bottom));
    navigate(&mut workspace, &mut state, 'h');
    assert_eq!(workspace.selection.focused(), Some(left));
}

#[test]
fn notification_panel_collects_hidden_windows_and_keeps_reports_after_close() {
    use ui::NotificationPanelAction as Action;
    let mut state = shell_state();
    while state.notification_dismiss_active_modal_without_response() {}
    let mut workspace = Workspace::new(None, false);
    let (clock, editor) = (SessionId(3), SessionId(4));
    workspace.windows.insert(clock, window(Size::new(106, 16)));
    let mut editor_window = window(Size::new(106, 16));
    editor_window.launch = WindowLaunch::Editor;
    workspace.windows.insert(editor, editor_window);
    workspace.selection.open(editor);

    let mut clock_host = shell_state();
    let mut editor_host = shell_state();
    use crate::session::window_launch::{WindowApplication, WindowOwner};
    clock_host.window_owner = WindowOwner::App(WindowApplication::Clock);
    editor_host.window_owner = WindowOwner::App(WindowApplication::Editor);
    let deliver =
        |ws: &mut Workspace, desktop: &mut ShellSession, window, host: &mut ShellSession| {
            let reports = host.app.take_notification_reports();
            assert_eq!(reports.len(), 1);
            for report in reports {
                ws.receive(
                    window,
                    AppToShell::NotificationReport(serde_json::to_value(report).unwrap()),
                    desktop,
                )
                .unwrap();
            }
        };
    clock_host.notify_alert_with_tone("Background clock", ui::NotificationTone::Error);
    editor_host.notify_alert_with_tone("Foreground app", ui::NotificationTone::Error);
    let editor_report_id = editor_host
        .app
        .notification_center()
        .active_report_id()
        .unwrap();
    deliver(&mut workspace, &mut state, clock, &mut clock_host);
    deliver(&mut workspace, &mut state, editor, &mut editor_host);
    workspace.remove(clock);
    state.open_status_details();
    let cards = state.notification_history.panel(false).cards;
    assert_eq!(cards.len(), 2);
    assert_eq!(cards[0].message, "Foreground app");
    assert_eq!(cards[1].message, "Background clock");
    assert_eq!(cards[0].source, WindowLaunch::Editor.title());
    assert_eq!(cards[1].source, WindowLaunch::Clock.title());
    state.activate_panel_action(Action::Dismiss(cards[0].id));
    assert_eq!(
        state.notification_history.pending_dismissals.get(&editor),
        Some(&vec![editor_report_id])
    );
    assert_eq!(state.notification_history.panel(false).cards[0], cards[1]);
    editor_host.notify_critical_modal("Critical", "New failure", vec![]);
    deliver(&mut workspace, &mut state, editor, &mut editor_host);
    assert!(!state.status_details_visible());
    state.open_status_details();
    state.activate_panel_action(Action::Clear(cards[1].id));
    let remaining = state.notification_history.panel(false).cards;
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].message, "New failure");
}

#[test]
#[cfg_attr(not(feature = "native-tests"), ignore = "requires local socket access")]
fn notification_panel_drains_the_last_report_when_the_window_exit_arrives_first() {
    let mut state = shell_state();
    let mut workspace = Workspace::new(None, false);
    let id = SessionId(7);
    let (mut window, peer, _listener) = connected_window(Size::new(106, 16));
    window.state = ProcessState::Exited {
        code: Some(0),
        forced: false,
    };
    window.close = CloseProgress::Exiting;
    workspace.windows.insert(id, window);
    workspace
        .drain_exited_window(id, Some(0), false, &mut state)
        .unwrap();
    assert!(workspace.windows.contains_key(&id));
    let report = app::NotificationReport {
        id: 1,
        tone: ui::NotificationTone::Error,
        title: None,
        message: "Last window report".into(),
    };
    let message = AppToShell::NotificationReport(serde_json::to_value(report).unwrap());
    let bytes = serde_json::to_vec(&message).unwrap();
    let mut peer = peer.into_inner();
    crate::process_host::event_loop::runtime().block_on(async {
        use tokio::io::AsyncWriteExt;
        peer.write_all(&(bytes.len() as u32).to_be_bytes())
            .await
            .unwrap();
        peer.write_all(&bytes).await.unwrap();
        peer.shutdown().await.unwrap();
    });
    drop(peer);
    let deadline = Instant::now() + Duration::from_secs(3);
    while workspace.windows.contains_key(&id) && Instant::now() < deadline {
        workspace
            .drain_exited_window(id, Some(0), false, &mut state)
            .unwrap();
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(!workspace.windows.contains_key(&id));
    assert_eq!(
        state.notification_history.panel(false).cards[0].message,
        "Last window report"
    );
}
