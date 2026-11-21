use super::*;

#[test]
fn explorer_task_error_detail_reports_the_failed_file_and_cause() {
    use app::explorer_tasks::{ExplorerItemFailure, ExplorerTaskError};
    let failures = [ExplorerItemFailure {
        source: PathBuf::from("/home/user/Documents/alpha.txt"),
        target: None,
        error: ExplorerTaskError::Platform(platform::PlatformError::InvalidInput {
            message: "Linux Trash ownership mismatch".into(),
        }),
    }];
    let detail = explorer_task_error_detail(None, &failures)
        .unwrap()
        .render_current();
    assert!(detail.contains("/home/user/Documents/alpha.txt"));
    assert!(detail.contains("Linux Trash ownership mismatch"));
    let fatal = ExplorerTaskError::Journal {
        message: "journal unavailable".into(),
    };
    assert_eq!(
        explorer_task_error_detail(Some(&fatal), &failures)
            .unwrap()
            .render_current(),
        fatal.to_string()
    );
    assert!(explorer_task_error_detail(None, &[]).is_none());
}

fn test_explorer_watchdog() -> watchdog::AppWatchdog {
    static WATCHDOG: std::sync::OnceLock<watchdog::AppWatchdog> = std::sync::OnceLock::new();
    WATCHDOG
        .get_or_init(|| {
            let _ = default_editor_watchdog();
            if let Some(process) = watchdog::ProcessWatchdog::global() {
                return process
                    .register_app(app::explorer_tasks::explorer_watchdog_descriptor())
                    .expect("Explorer workflow watchdog registration");
            }
            let root = std::env::temp_dir().join(format!(
                "tundra-shell-explorer-watchdog-tests-{}",
                std::process::id()
            ));
            let config = watchdog::WatchdogConfig::new(
                root.join("reports"),
                root.join("fallback"),
                root.join("state"),
                "tundra-shell-tests",
                env!("CARGO_PKG_VERSION"),
            );
            let (runtime, process) =
                watchdog::WatchdogRuntime::start(config).expect("Explorer workflow test watchdog");
            let process = process
                .install_global()
                .expect("install Explorer workflow test watchdog");
            let _runtime = Box::leak(Box::new(runtime));
            process
                .register_app(app::explorer_tasks::explorer_watchdog_descriptor())
                .expect("Explorer workflow watchdog registration")
        })
        .clone()
}

#[test]
fn completed_move_from_an_old_session_cannot_change_new_clipboard_or_undo_history() {
    let fixture = std::env::temp_dir().join(format!(
        "tundra-shell-explorer-stale-move-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
    ));
    let source_root = fixture.join("source");
    let destination = fixture.join("destination");
    std::fs::create_dir_all(&source_root).unwrap();
    std::fs::create_dir(&destination).unwrap();
    let source = source_root.join("old.txt");
    std::fs::write(&source, "old session move").unwrap();
    let storage = storage_at(&fixture);
    let runtime = ShellExplorerTaskRuntime::new_managed(storage.clone(), test_explorer_watchdog());
    let id = runtime
        .submit(
            ExplorerTaskPlan::Transfer(ExplorerTransferPlan::new(
                ExplorerTransferOperation::Move,
                vec![source],
                &destination,
            )),
            ShellExplorerTaskKind::Move {
                from_clipboard: true,
            },
            "same-user".into(),
        )
        .unwrap();
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    let mut actor = AuthSession {
        source: identity::IdentitySource::SystemCurrentProcess,
        session_id: "old-session".into(),
        user_id: "same-user".into(),
        username: "tester".into(),
        role: UserRole::Admin,
        started_at_epoch_ms: 1,
    };
    state.app.dispatch_at(
        app::AppCommand::SetAuthSession(Some(actor.clone())),
        Instant::now(),
    );
    state.enter_new_page(ShellScreen::Explorer);
    state.explorer_task_runtime = Some(runtime.clone());
    state.tasks.register(
        TaskDomain::Explorer,
        id.0,
        TaskOwner::Session,
        TaskClass::Operation,
        TaskControl::new(|| {}, || true),
    );
    state.app.explorer_operation_started().unwrap();
    let summary = wait_for_summary(&runtime);
    assert_eq!(summary.undo_moves.as_ref().map(Vec::len), Some(1));
    state.tasks.reset();
    state
        .app
        .dispatch_at(app::AppCommand::SetAuthSession(None), Instant::now());
    actor.session_id = "new-session".into();
    state
        .app
        .dispatch_at(app::AppCommand::SetAuthSession(Some(actor)), Instant::now());
    let fresh = source_root.join("new.txt");
    std::fs::write(&fresh, "new session clipboard").unwrap();
    state.replace_explorer_state(Some(ExplorerState::new(&source_root, true)));
    let platform = platform::native_platform();
    for command in [ExplorerCommand::Refresh, ExplorerCommand::Cut] {
        state
            .app
            .dispatch_explorer_at(command, platform.as_ref(), &storage, Instant::now());
    }
    state.apply_explorer_task_event(
        ExplorerTaskEvent::Finished { id, summary },
        platform.as_ref(),
    );
    let explorer = state.app.explorer_state().unwrap();
    assert_eq!(explorer.clipboard.as_ref().unwrap().paths, [fresh]);
    assert!(!explorer.can_undo);
    assert!(destination.join("old.txt").exists());
    drop(state);
    drop(runtime);
    std::fs::remove_dir_all(fixture).unwrap();
}

#[test]
fn shell_runtime_executes_copy_on_real_temporary_paths() {
    let fixture = std::env::temp_dir().join(format!(
        "tundra-shell-explorer-task-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let source_root = fixture.join("source");
    let destination = fixture.join("destination");
    std::fs::create_dir_all(&source_root).expect("source directory");
    std::fs::create_dir_all(&destination).expect("destination directory");
    let source = source_root.join("note.txt");
    std::fs::write(&source, b"background copy").expect("source file");

    let storage = storage_at(&fixture);
    let runtime = ShellExplorerTaskRuntime::new_managed(storage, test_explorer_watchdog());
    let plan =
        ExplorerTransferPlan::new(ExplorerTransferOperation::Copy, vec![source], &destination);
    runtime
        .submit(
            ExplorerTaskPlan::Transfer(plan),
            ShellExplorerTaskKind::Copy,
            "Tester".to_string(),
        )
        .expect("task accepted");

    let summary = wait_for_summary(&runtime);

    assert_eq!(summary.succeeded_items, 1);
    assert_eq!(summary.failed_items, 0);
    assert_eq!(
        std::fs::read(destination.join("note.txt")).expect("copied file"),
        b"background copy"
    );
    drop(runtime);
    let _ = std::fs::remove_dir_all(fixture);
}

#[test]
fn recursive_conflict_preflight_reports_only_non_merge_targets() {
    let fixture = std::env::temp_dir().join(format!(
        "tundra-shell-explorer-conflicts-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    let source = fixture.join("source").join("folder");
    let destination = fixture.join("destination");
    let target = destination.join("folder");
    std::fs::create_dir_all(source.join("sub")).expect("source directories");
    std::fs::create_dir_all(target.join("sub")).expect("target directories");
    std::fs::write(source.join("same.txt"), b"new").expect("source collision");
    std::fs::write(target.join("same.txt"), b"old").expect("target collision");
    std::fs::write(source.join("only.txt"), b"only source").expect("source-only file");
    std::fs::write(source.join("sub").join("deep.txt"), b"new deep")
        .expect("deep source collision");
    std::fs::write(target.join("sub").join("deep.txt"), b"old deep")
        .expect("deep target collision");
    let canonical_destination = std::fs::canonicalize(&destination).expect("canonical destination");
    let canonical_target = canonical_destination.join("folder");
    let platform = platform::native_platform();
    let mut conflicts = Vec::new();

    collect_explorer_conflicts_no_follow(
        platform.as_ref(),
        &source,
        &canonical_target,
        &mut conflicts,
    )
    .expect("recursive conflict scan");

    assert_eq!(conflicts.len(), 2);
    assert!(
        conflicts
            .iter()
            .any(|(_, target)| target == &canonical_target.join("same.txt"))
    );
    assert!(
        conflicts
            .iter()
            .any(|(_, target)| { target == &canonical_target.join("sub").join("deep.txt") })
    );
    assert!(
        !conflicts
            .iter()
            .any(|(_, target)| target == &canonical_target)
    );
    let _ = std::fs::remove_dir_all(fixture);
}

fn storage_at(fixture: &Path) -> StorageManager {
    StorageManager::open(app_paths_at(fixture))
        .expect("test storage opens")
        .manager
}

fn app_paths_at(fixture: &Path) -> platform::AppPaths {
    platform::AppPaths::from_parts(
        fixture.join("config.toml"),
        fixture.join("data"),
        fixture.join("cache"),
        fixture.join("logs"),
        fixture.join("temp"),
    )
    .expect("absolute test app paths")
}

fn wait_for_summary(
    runtime: &ShellExplorerTaskRuntime,
) -> app::explorer_tasks::ExplorerTaskSummary {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(summary) = runtime.drain_events().into_iter().find_map(|event| {
            if let ExplorerTaskEvent::Finished { summary, .. } = event {
                Some(summary)
            } else {
                None
            }
        }) {
            return summary;
        }
        assert!(Instant::now() < deadline, "background task timed out");
        std::thread::sleep(Duration::from_millis(5));
    }
}
