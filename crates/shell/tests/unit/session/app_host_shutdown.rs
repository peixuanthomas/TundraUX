use std::path::Path;

use super::*;
use watchdog::{WatchdogConfig, WatchdogRuntime};
use workspace_protocol::{ServiceError, ServiceSessionSnapshot};

struct TestDirectory(PathBuf);

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn host(watchdog: AppWatchdog, paths: &platform::AppPaths) -> AppHostSession {
    let now = Instant::now();
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    state.open_editor();
    state.storage_manager = Some(
        storage::StorageManager::open(paths.clone())
            .unwrap()
            .manager,
    );
    state.app.dispatch_at(
        app::AppCommand::SetAuthSession(Some(AuthSession {
            source: identity::IdentitySource::SystemCurrentProcess,
            session_id: "host-shutdown".into(),
            user_id: "alice".into(),
            username: "alice".into(),
            role: identity::UserRole::User,
            started_at_epoch_ms: 1,
        })),
        now,
    );
    let redraw = RedrawScheduler::new(now, RedrawIdentity::from_session(&state), false);
    let storage = state.storage_manager.clone();
    let last_storage_config_revision = state.app.storage_config_revision();
    let unavailable = Err(ServiceError::Unavailable);
    AppHostSession {
        state,
        platform: Arc::new(platform::mock::UnsupportedPlatform),
        command_lines: CommandLineHosts::new(watchdog.clone()),
        vim: vim_host::EditorVimHost::new(watchdog.clone()),
        scripts: script_host::ScriptHosts::new(watchdog.clone()),
        watchdog,
        services: SessionServicesClient::for_test(ServiceSessionSnapshot {
            authorization: unavailable.clone(),
            config: unavailable.clone(),
            profile: unavailable.clone(),
            clock_profile: unavailable.clone(),
            clock_time: unavailable.clone(),
            files: unavailable.clone(),
            system: unavailable.clone(),
            system_requests: unavailable,
        }),
        compositor: ScreenCompositor::default(),
        terminal: Terminal::new(TestBackend::new(120, 40)).unwrap(),
        theme: ui::TundraTheme::default_dark(),
        render_capabilities: ui::RenderCapabilities::default(),
        theme_reloader: UserThemeReloader::new(storage, now),
        redraw,
        next_service_sync: now,
        last_background_poll: now,
        last_storage_config_revision,
        status_dirty: true,
        size: Size::new(120, 38),
        layout: LayoutGeneration(1),
        visible: true,
        focused: true,
        frozen: false,
        close_status: WindowClose::Idle,
        last_status: None,
        pending_explorer_selection: None,
        output: VecDeque::new(),
        input_fences: InputFences::default(),
        frame_sequence: 0,
    }
}

#[test]
fn scrolling_explorer_clears_wide_characters_from_shorter_type_labels() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let paths = platform::AppPaths::from_parts(
        root.join("config"),
        root.join("data"),
        root.join("cache"),
        root.join("logs"),
        root.join("temp"),
    )
    .unwrap();
    let (_runtime, process) = WatchdogRuntime::start_isolated(WatchdogConfig::new(
        root.join("reports"),
        root.join("fallback"),
        root.join("watchdog"),
        "app-host-explorer-scroll-test",
        "test",
    ))
    .unwrap();
    let watchdog = process
        .register_app(AppDescriptor::new(
            AppId::from_static("app-host-explorer-scroll-test"),
            "AppHost Explorer scroll test",
            "test",
            AppCriticality::Optional,
        ))
        .unwrap();
    let files = root.join("files");
    std::fs::create_dir_all(files.join("a-folder")).unwrap();
    std::fs::create_dir(files.join("b-folder")).unwrap();
    for name in ["01.xlsx", "02.pdf"] {
        std::fs::write(files.join(name), []).unwrap();
    }
    for index in 0..40 {
        std::fs::write(files.join(format!("tail-{index:02}.txt")), []).unwrap();
    }

    let mut host = host(watchdog, &paths);
    let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("../ascii-assets/assets");
    host.state.language = Arc::new(
        i18n::LanguageSnapshot::load(assets, "zh-CN", 1)
            .unwrap()
            .snapshot,
    );
    let _language = i18n::enter_snapshot(host.state.language.clone());
    host.state.window_owner = WindowOwner::App(window_launch::WindowApplication::Explorer);
    host.state.reset_navigation(ShellScreen::Home);
    let storage = host.state.storage_manager.clone().unwrap();
    host.state.open_explorer_at(
        host.platform.as_ref(),
        &storage,
        files,
        ExplorerPurpose::Browse,
    );

    for (offset, expected) in [(3, "PDF 文件"), (2, "XLSX 文件"), (1, "文件夹")] {
        host.state.update_explorer_state(|explorer| {
            explorer.viewport_offset = offset;
            explorer.viewport_follows_focus = false;
        });
        host.redraw.request_input_redraw();
        let frame = host.frame(Instant::now()).unwrap().unwrap();
        let bounds = Rect::new(0, 0, host.size.width, host.size.height + 2);
        let ui::ShellLayout::Full { main, .. } = host.state.shell_layout_for(bounds) else {
            panic!("expected full shell layout");
        };
        let layout = ui::explorer_layout(main, &host.state.to_explorer_view_model());
        let column = layout
            .columns
            .iter()
            .find(|column| column.column == ui::ExplorerSortColumn::Type)
            .unwrap()
            .area;
        let start = usize::from(layout.rows[0].area.y - 1) * usize::from(frame.size.width)
            + usize::from(column.x);
        let label: String = frame.cells[start..start + usize::from(column.width)]
            .iter()
            .filter(|cell| !cell.skip)
            .map(|cell| cell.symbol.as_str())
            .collect();
        assert_eq!(label.split('|').next().unwrap().trim_end(), expected);
    }
}

#[test]
fn app_host_keeps_the_editor_visible_after_an_image_open_fails() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let paths = platform::AppPaths::from_parts(
        root.join("config"),
        root.join("data"),
        root.join("cache"),
        root.join("logs"),
        root.join("temp"),
    )
    .unwrap();
    let (runtime, process) = WatchdogRuntime::start_isolated(WatchdogConfig::new(
        root.join("reports"),
        root.join("fallback"),
        root.join("watchdog"),
        "app-host-editor-load-error-test",
        "test",
    ))
    .unwrap();
    let watchdog = process
        .register_app(AppDescriptor::new(
            AppId::from_static("app-host-editor-load-error-test"),
            "AppHost editor load error test",
            "test",
            AppCriticality::Optional,
        ))
        .unwrap();
    let broken = root.join("broken.png");
    let document = root.join("retry.txt");
    std::fs::write(&broken, b"invalid image").unwrap();
    std::fs::write(&document, "a readable document").unwrap();

    let mut host = host(watchdog.clone(), &paths);
    host.state.window_owner =
        WindowOwner::App(crate::session::window_launch::WindowApplication::Editor);
    host.state.reset_navigation(ShellScreen::Home);
    host.state.editor_task_runtime = ShellEditorTaskRuntime::new_managed(watchdog);
    host.compositor.capture_enabled = true;
    host.services.acknowledge_updates();
    host.theme_reloader = UserThemeReloader::new(None, Instant::now());
    host.next_service_sync = Instant::now() + Duration::from_secs(60);
    while host
        .state
        .notification_dismiss_active_modal_without_response()
    {}

    let start_load = |host: &mut AppHostSession, path: &Path| {
        WindowLaunch::EditorPath(path.as_os_str().to_owned())
            .activate(&mut host.state, host.platform.as_ref());
        assert!(host.state.pages.editor.editor_load_state.is_some());
    };
    let wait_for_load = |host: &mut AppHostSession| {
        let deadline = Instant::now() + Duration::from_secs(5);
        while host.state.pages.editor.editor_load_state.is_some() {
            host.poll(Instant::now()).unwrap();
            assert!(Instant::now() < deadline, "editor load did not finish");
            std::thread::sleep(HOST_TICK);
        }
    };

    start_load(&mut host, &broken);
    let editor = host.state.content_page_id();
    wait_for_load(&mut host);
    assert_eq!(host.state.content_screen(), ShellScreen::Editor);
    assert_eq!(host.state.content_page_id(), editor);
    assert_eq!(host.state.window_close, WindowClose::Idle);
    assert_eq!(host.close_status, WindowClose::Idle);
    let reason = host
        .state
        .to_shell_chrome_view_model()
        .status
        .error
        .unwrap();
    assert!(reason.contains("broken.png") && reason.contains("Could not preview this image"));

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let now = Instant::now();
        host.poll(now).unwrap();
        if let Some(frame) = host.frame(now).unwrap() {
            assert!(frame.status.contains(&reason));
            break;
        }
        assert!(now < deadline, "AppHost did not emit the error frame");
        std::thread::sleep(HOST_TICK);
    }

    start_load(&mut host, &document);
    wait_for_load(&mut host);
    assert_eq!(host.state.to_editor_view_model().file_name, "retry.txt");
    assert!(
        host.state
            .to_shell_chrome_view_model()
            .status
            .error
            .is_none()
    );
    start_load(&mut host, &broken);
    wait_for_load(&mut host);
    assert_eq!(host.state.content_page_id(), editor);
    assert_eq!(host.state.to_editor_view_model().file_name, "retry.txt");

    host.state.reset_navigation(ShellScreen::Home);
    start_load(&mut host, &broken);
    host.receive(ShellToApp::Input {
        serial: 1,
        layout: host.layout,
        event: serde_json::to_value(InputEvent::key(InputKey::Escape)).unwrap(),
    })
    .unwrap();
    host.poll(Instant::now()).unwrap();
    assert_eq!(host.state.content_screen(), ShellScreen::Home);
    assert_eq!(host.state.window_close, WindowClose::Confirmed);

    host.shutdown().unwrap();
    drop(host);
    drop(process);
    runtime.shutdown().unwrap();
}

#[test]
fn app_host_polls_pdf_on_open_and_resize_and_uses_warm_pages_on_the_first_input_frame() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().canonicalize().unwrap();
    let paths = platform::AppPaths::from_parts(
        root.join("config"),
        root.join("data"),
        root.join("cache"),
        root.join("logs"),
        root.join("temp"),
    )
    .unwrap();
    let (runtime, process) = WatchdogRuntime::start_isolated(WatchdogConfig::new(
        root.join("reports"),
        root.join("fallback"),
        root.join("watchdog"),
        "app-host-pdf-render-test",
        "test",
    ))
    .unwrap();
    let watchdog = process
        .register_app(AppDescriptor::new(
            AppId::from_static("app-host-pdf-render-test"),
            "AppHost PDF render test",
            "test",
            AppCriticality::Optional,
        ))
        .unwrap();
    let path = root.join("sample.PDF");
    std::fs::write(
        &path,
        include_bytes!("../../../../app/tests/fixtures/pdf/preview.pdf"),
    )
    .unwrap();
    let mut host = host(watchdog.clone(), &paths);
    host.state.window_owner =
        WindowOwner::App(crate::session::window_launch::WindowApplication::Editor);
    host.state.editor_task_runtime = ShellEditorTaskRuntime::new_managed(watchdog);
    host.compositor.capture_enabled = true;
    host.compositor.capture_workspace_images = true;
    host.services.acknowledge_updates();
    host.theme_reloader = UserThemeReloader::new(None, Instant::now());
    host.next_service_sync = Instant::now() + Duration::from_secs(60);
    while host
        .state
        .notification_dismiss_active_modal_without_response()
    {}
    WindowLaunch::EditorPath(path.into()).activate(&mut host.state, host.platform.as_ref());
    assert!(host.state.pages.editor.editor_load_state.is_some());

    let wait_for_image = |host: &mut AppHostSession, after: u64, input: Option<u64>| {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let now = Instant::now();
            host.poll(now).unwrap();
            if let Some(frame) = host.frame(now).unwrap()
                && input.is_none_or(|serial| frame.processed_input_serial >= serial)
            {
                assert!(
                    !frame.modal,
                    "Unexpected test overlay: {:?}",
                    host.state.to_notification_view_model()
                );
                if let Some(image) = frame
                    .images
                    .into_iter()
                    .find(|image| image.image_id > after)
                {
                    return image;
                }
                assert!(
                    input.is_none(),
                    "warm PDF page was missing from the first input frame"
                );
            }
            assert!(
                now < deadline,
                "AppHost did not publish the requested PDF page after image {after}"
            );
            std::thread::sleep(HOST_TICK);
        }
    };

    let first = wait_for_image(&mut host, 0, None);
    let deadline = Instant::now() + Duration::from_secs(5);
    while host.state.editor_pdf_has_background_work() {
        host.poll(Instant::now()).unwrap();
        assert!(
            Instant::now() < deadline,
            "AppHost did not settle PDF prefetch"
        );
        std::thread::sleep(HOST_TICK);
    }
    let press = |host: &mut AppHostSession, serial, key| {
        host.receive(ShellToApp::Input {
            serial,
            layout: host.layout,
            event: serde_json::to_value(InputEvent::key(key)).unwrap(),
        })
        .unwrap();
    };
    press(&mut host, 1, InputKey::PageDown);
    let scan = wait_for_image(&mut host, first.image_id, Some(1));
    assert!(
        scan.rgba
            .chunks_exact(4)
            .any(|pixel| pixel == [162, 40, 20, 255])
    );
    press(&mut host, 2, InputKey::PageUp);
    let again = wait_for_image(&mut host, 0, Some(2));
    assert_eq!(again.image_id, first.image_id);
    assert!(Arc::ptr_eq(&again.rgba, &first.rgba));
    press(&mut host, 3, InputKey::PageDown);
    host.receive(ShellToApp::Viewport {
        layout: LayoutGeneration(2),
        size: Some(MIN_CONTENT_SIZE),
        visible: true,
        focused: true,
    })
    .unwrap();
    let resized = wait_for_image(&mut host, scan.image_id, None);
    assert!(resized.width < scan.width && resized.height < scan.height);
    assert_eq!(host.state.to_editor_view_model().pdf.unwrap().page, 1);

    host.shutdown().unwrap();
    drop(host);
    drop(process);
    runtime.shutdown().unwrap();
}

#[test]
fn an_explicit_debug_request_is_forwarded_before_close_without_panicking_the_host() {
    let directory = TestDirectory(std::env::temp_dir().join(format!(
        "tundra-app-debug-{}-{}",
        std::process::id(),
        PageInstanceId::new().get()
    )));
    std::fs::create_dir_all(&directory.0).unwrap();
    let paths = platform::AppPaths::from_parts(
        directory.0.join("config"),
        directory.0.join("data"),
        directory.0.join("cache"),
        directory.0.join("logs"),
        directory.0.join("temp"),
    )
    .unwrap();
    let (runtime, process) = WatchdogRuntime::start_isolated(WatchdogConfig::new(
        directory.0.join("reports"),
        directory.0.join("fallback"),
        directory.0.join("watchdog"),
        "app-debug-route-test",
        "test",
    ))
    .unwrap();
    let watchdog = process
        .register_app(AppDescriptor::new(
            AppId::from_static("app-host"),
            "AppHost",
            "test",
            AppCriticality::Optional,
        ))
        .unwrap();
    let mut host = host(watchdog, &paths);
    host.command_line_event(CommandLineHostEvent::PanicRequested);
    assert_eq!(host.close_status, WindowClose::Idle);
    assert_eq!(host.state.window_close, WindowClose::Idle);
    host.receive(ShellToApp::RequestClose).unwrap();
    assert_eq!(
        host.output.pop_front(),
        Some(AppToShell::ShellAction(HostAction::DebugPanicRequested))
    );
    assert_eq!(host.output.pop_front(), Some(AppToShell::CloseDeferred));
    assert!(host.output.is_empty());
    host.shutdown().unwrap();
    drop(host);
    drop(process);
    runtime.shutdown().unwrap();
}

#[test]
fn shutdown_preserves_latest_dirty_text_and_confirmed_discard_removes_it() {
    let temp = std::env::temp_dir().canonicalize().unwrap();
    for discard in [false, true] {
        let directory = TestDirectory(temp.join(format!(
            "tundra-app-host-shutdown-{}-{}",
            std::process::id(),
            PageInstanceId::new().get()
        )));
        std::fs::create_dir_all(&directory.0).unwrap();
        let paths = platform::AppPaths::from_parts(
            directory.0.join("config"),
            directory.0.join("data"),
            directory.0.join("cache"),
            directory.0.join("logs"),
            directory.0.join("temp"),
        )
        .unwrap();
        let (runtime, process) = WatchdogRuntime::start_isolated(WatchdogConfig::new(
            directory.0.join("reports"),
            directory.0.join("fallback"),
            directory.0.join("watchdog-data"),
            "app-host-shutdown-test",
            env!("CARGO_PKG_VERSION"),
        ))
        .unwrap();
        let watchdog = process
            .register_app(AppDescriptor::new(
                AppId::from_static("app-host-shutdown-test"),
                "AppHost shutdown test",
                env!("CARGO_PKG_VERSION"),
                AppCriticality::Optional,
            ))
            .unwrap();
        let mut host = host(watchdog, &paths);
        host.state.apply_editor_command(
            app::editor::EditorCommand::InsertText("earlier draft".into()),
            host.platform.as_ref(),
        );
        assert!(host.state.persist_all_editor_recovery_now(Instant::now()));
        let recovery = host
            .state
            .pages
            .editor
            .editor_recovery_lease
            .as_ref()
            .unwrap()
            .path()
            .to_owned();
        host.state.apply_editor_command(
            app::editor::EditorCommand::InsertText(" plus latest GUI edits".into()),
            host.platform.as_ref(),
        );
        if discard {
            let page = host.state.active_page_id();
            let revision = host.state.app.editor_state().unwrap().revision();
            host.state
                .approved_editor_closes
                .insert(page, (Some(revision), true));
            host.state.window_close = WindowClose::Confirmed;
            host.close_status = WindowClose::Confirmed;
        }

        host.shutdown().unwrap();

        if discard {
            assert!(
                !recovery.exists(),
                "Explicit discard must not leave a draft"
            );
        } else {
            let record: serde_json::Value =
                serde_json::from_slice(&std::fs::read(&recovery).unwrap()).unwrap();
            assert_eq!(
                record["payload"]["text"],
                "earlier draft plus latest GUI edits"
            );
            assert!(host.state.app.editor_state().unwrap().is_dirty());
        }
        drop(host);
        drop(process);
        runtime.shutdown().unwrap();
        assert_eq!(recovery.exists(), !discard);
    }
}
