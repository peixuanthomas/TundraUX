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
        launcher_icons: None,
        terminal: Terminal::new(TestBackend::new(120, 40)).unwrap(),
        theme: ui::TundraTheme::default_dark(),
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
    for discard in [false, true] {
        let directory = TestDirectory(std::env::temp_dir().join(format!(
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
