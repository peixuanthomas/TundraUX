use super::*;

#[test]
fn editor_load_allows_covering_page_and_failure_restores_only_its_origin() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    state.set_navigation_path(vec![ShellScreen::Home, ShellScreen::Explorer]);
    let rollback = state.begin_editor_navigation(false);
    let operation = EditorLoadOperation::Open {
        navigation: EditorLoadNavigation::Explorer,
        rollback,
        reload: None,
        replacing_dirty: false,
    };
    state.pages.editor.editor_load_state = Some(EditorLoadState {
        id: 17,
        path: PathBuf::from("large.log"),
        stage: EditorTaskStage::Reading,
        completed_bytes: 1,
        total_bytes: Some(2),
        operation: operation.clone(),
    });
    state.refresh_hit_map();
    let clock = hit_region_center(&state, ShellComponent::ClockButton);

    state.apply_input(InputEvent::mouse_down(PointerButton::Left, clock));

    assert_eq!(state.active_screen(), ShellScreen::Clock);
    let clock_page = state.active_page_id();
    let clock_focus = state.pages.common.focused_component;
    assert_eq!(
        state.screen_stack(),
        vec![
            ShellScreen::Home,
            ShellScreen::Explorer,
            ShellScreen::Editor,
            ShellScreen::Clock
        ]
    );
    state.pages.editor.editor_load_state = None;
    let pages = state.live_page_ids();
    let loading_editor = pages[pages.len() - 2];
    state.with_page_context(loading_editor, |s| {
        s.restore_editor_load_navigation(&operation)
    });
    assert_eq!(state.active_page_id(), clock_page);
    assert_eq!(state.pages.common.focused_component, clock_focus);
    assert_eq!(
        state.screen_stack(),
        vec![ShellScreen::Home, ShellScreen::Explorer, ShellScreen::Clock]
    );
    state.close_page_instance(clock_page);
    assert_eq!(state.active_screen(), ShellScreen::Explorer);
    assert_eq!(
        state.pages.common.focused_component,
        ShellComponent::Explorer
    );
}

#[test]
fn cancelled_editor_loads_release_the_concurrency_permit() {
    let runtime = ShellEditorTaskRuntime::new();
    let path = std::env::temp_dir().join(format!(
        "tundra-editor-cancel-permit-{}-{}.txt",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    std::fs::write(&path, b"cancel me").expect("seed editor load fixture");
    let first = next_editor_task_id();
    let second = next_editor_task_id();
    for id in [first, second] {
        runtime
            .submit_load(id, path.clone(), EditorTaskAccess::Editable)
            .expect("submit load");
        runtime.cancel(id);
    }

    let mut terminal_events = 0;
    for _ in 0..400 {
        terminal_events += runtime
            .drain_events()
            .into_iter()
            .filter(|event| matches!(event, EditorTaskEvent::LoadFinished { .. }))
            .count();
        if terminal_events == 2 {
            break;
        }
        std::thread::yield_now();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(terminal_events, 2);
    assert_eq!(runtime.shared.active_loads.load(Ordering::Acquire), 0);
    assert_eq!(runtime.shared.active_load_bytes.load(Ordering::Acquire), 0);

    let third = next_editor_task_id();
    runtime
        .submit_load(third, path.clone(), EditorTaskAccess::Editable)
        .expect("a later load must not be blocked by leaked permits");
    runtime.cancel(third);
    let _ = std::fs::remove_file(path);
}

#[test]
fn editor_load_byte_budget_is_shared_and_released() {
    let active = AtomicU64::new(0);
    let first = reserve_editor_load_bytes(&active, 700 * 1024 * 1024)
        .expect("first document fits the shared budget");
    assert!(reserve_editor_load_bytes(&active, 400 * 1024 * 1024).is_err());
    drop(first);
    let maximum = reserve_editor_load_bytes(&active, platform::MAX_DOCUMENT_BYTES)
        .expect("one maximum-sized document fits");
    assert_eq!(active.load(Ordering::Acquire), platform::MAX_DOCUMENT_BYTES);
    drop(maximum);
    assert_eq!(active.load(Ordering::Acquire), 0);
}

#[test]
fn stale_save_completion_does_not_mark_a_replacement_document_saved() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    let mut replacement = EditorState::untitled(app::editor::DocumentKind::PlainText);
    replacement.apply(app::editor::EditorCommand::InsertText(
        "replacement".to_string(),
    ));
    let expected = replacement.clone();
    state.pages.editor.editor_document_generation = 9;
    state.app.dispatch_at(
        app::AppCommand::SetEditorState(Some(replacement)),
        Instant::now(),
    );
    state.pages.editor.editor_close_after_save = true;
    state.pages.editor.editor_open_after_save = true;

    state.finish_editor_save(
        EditorSaveState {
            id: 1,
            path: PathBuf::from("old.txt"),
            document_generation: 8,
            revision: 1,
            stage: EditorTaskStage::Writing,
        },
        Ok(DocumentFingerprint {
            len: 3,
            modified: None,
            content_hash: 7,
        }),
        &platform::mock::UnsupportedPlatform,
    );

    assert_eq!(state.app.editor_state(), Some(&expected));
    assert_eq!(state.pages.editor.editor_fingerprint, None);
    assert!(!state.pages.editor.editor_close_after_save);
    assert!(!state.pages.editor.editor_open_after_save);
}

#[test]
fn shutdown_waits_for_an_active_editor_save() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    state.enter_new_page(ShellScreen::Editor);
    state.enter_new_page(ShellScreen::Clock);
    state.pages.editor.editor_save_state = Some(EditorSaveState {
        id: 5,
        path: PathBuf::from("large.txt"),
        document_generation: state.pages.editor.editor_document_generation,
        revision: 1,
        stage: EditorTaskStage::Writing,
    });

    assert_eq!(state.apply_input(InputEvent::Shutdown), ShellAction::Redraw);
    assert!(!state.shutdown_requested);
    assert!(state.pages.editor.editor_save_state.is_some());

    state.pages.editor.editor_save_state = None;
    assert_eq!(state.apply_input(InputEvent::Shutdown), ShellAction::Exit);
    assert!(state.shutdown_requested);
}
