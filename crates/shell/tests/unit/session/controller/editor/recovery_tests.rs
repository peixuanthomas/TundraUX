use super::*;

#[test]
fn recovery_failures_back_off_without_marking_the_draft_saved() {
    for acquire_fails in [true, false] {
        let root = std::env::temp_dir().join(format!(
            "tundra-editor-recovery-retry-{}-{}",
            std::process::id(),
            crate::session::controller::editor::tasks::next_editor_task_id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let root = std::fs::canonicalize(root).unwrap();
        let paths = platform::AppPaths::from_parts(
            root.join("config"),
            root.join("data"),
            root.join("cache"),
            root.join("logs"),
            root.join("temp"),
        )
        .unwrap();
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
                source: identity::IdentitySource::LocalAccount,
                session_id: "recovery-retry".into(),
                user_id: "alice".into(),
                username: "alice".into(),
                role: identity::UserRole::User,
                started_at_epoch_ms: 1,
            })),
            Instant::now(),
        );
        state.apply_editor_command(
            app::editor::EditorCommand::InsertText("dirty draft".into()),
            &platform::mock::UnsupportedPlatform,
        );
        let failed_at =
            state.pages.editor.editor_recovery_dirty_since.unwrap() + EDITOR_RECOVERY_IDLE;
        let blocked = if acquire_fails {
            let directory =
                app::editor_recovery::editor_recovery_path(&paths, "alice").with_extension("");
            std::fs::create_dir_all(directory.parent().unwrap()).unwrap();
            std::fs::write(&directory, "blocks recovery directory creation").unwrap();
            directory
        } else {
            let lease = app::editor_recovery::acquire_editor_recovery(
                &paths,
                "alice",
                &state.pages.editor.editor_recovery_id,
            )
            .unwrap();
            let path = lease.path().to_owned();
            std::fs::create_dir(&path).unwrap();
            state.pages.editor.editor_recovery_lease = Some(lease);
            path
        };

        assert!(state.persist_editor_recovery_if_due(failed_at));
        let retry_at = failed_at + EDITOR_RECOVERY_INTERVAL;
        assert!(state.pages.editor.editor_last_recovery_write.is_none());
        assert!(state.app.editor_state().unwrap().is_dirty());
        assert!(state.pages.editor.editor_message.is_some());
        assert_eq!(
            state.background_timer_timeout(failed_at, EDITOR_RECOVERY_INTERVAL),
            EDITOR_RECOVERY_INTERVAL
        );
        state.apply_editor_command(
            app::editor::EditorCommand::InsertText(" plus edits".into()),
            &platform::mock::UnsupportedPlatform,
        );
        assert_eq!(
            state.background_timer_timeout(failed_at, EDITOR_RECOVERY_INTERVAL),
            EDITOR_RECOVERY_INTERVAL
        );
        if acquire_fails {
            std::fs::remove_file(&blocked).unwrap();
        } else {
            std::fs::remove_dir(&blocked).unwrap();
        }
        let just_before = retry_at - Duration::from_millis(1);
        assert!(!state.persist_editor_recovery_if_due(just_before));
        assert!(state.pages.editor.editor_last_recovery_write.is_none());
        assert_eq!(
            state.background_timer_timeout(just_before, EDITOR_RECOVERY_INTERVAL),
            Duration::from_millis(1)
        );
        assert!(!state.persist_editor_recovery_if_due(retry_at));
        assert_eq!(
            state.pages.editor.editor_last_recovery_write,
            Some(retry_at)
        );
        assert!(state.app.editor_state().unwrap().is_dirty());
        let recovery_path = state
            .pages
            .editor
            .editor_recovery_lease
            .as_ref()
            .unwrap()
            .path()
            .to_owned();
        assert!(
            std::fs::read_to_string(&recovery_path)
                .unwrap()
                .contains("dirty draft plus edits")
        );

        std::fs::remove_file(&recovery_path).unwrap();
        std::fs::create_dir(&recovery_path).unwrap();
        assert!(!state.persist_editor_recovery_now(retry_at));
        assert_eq!(
            state.pages.editor.editor_last_recovery_write,
            Some(retry_at)
        );
        std::fs::remove_dir(&recovery_path).unwrap();
        assert!(state.persist_editor_recovery_now(retry_at + Duration::from_millis(1)));
        assert_eq!(
            state.pages.editor.editor_last_recovery_write,
            Some(retry_at + Duration::from_millis(1))
        );
        drop(state);
        std::fs::remove_dir_all(root).unwrap();
    }
}
