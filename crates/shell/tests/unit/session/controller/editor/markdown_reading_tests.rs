use super::*;
use app::editor::{EditorCommand, EditorMode, EditorPosition, EditorViewport};

const PLATFORM: platform::mock::UnsupportedPlatform = platform::mock::UnsupportedPlatform;

fn session(path: impl Into<PathBuf>, source: &[u8]) -> ShellSession {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 36),
        ShellHomeMode::User,
    );
    state.open_editor();
    state.set_editor_vim_enabled(false);
    state.app.dispatch_at(
        app::AppCommand::SetEditorState(Some(EditorState::open(path, source).unwrap())),
        Instant::now(),
    );
    state.refresh_hit_map();
    state
}

fn poll_until(state: &mut ShellSession, condition: impl Fn(&ShellSession) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition(state) {
        state.poll_editor_background_tasks(&PLATFORM);
        assert!(
            Instant::now() < deadline,
            "Editor background work did not settle"
        );
        std::thread::yield_now();
    }
}

fn mouse(state: &mut ShellSession, point: CellPosition, kind: ui::MouseEventKind) {
    state.apply_input_with_platform(
        InputEvent::mouse(MouseInput::new(point.0, point.1, kind)),
        &PLATFORM,
    );
}

#[test]
fn standard_editor_reads_the_unsaved_draft_and_restores_source() {
    let mut state = session("guide.MD", b"original");
    {
        let editor = state.app.editor_state().unwrap();
        assert_eq!(editor.mode, EditorMode::Source);
        assert_eq!(state.to_editor_view_model().markdown_reading, Some(false));
    }
    let draft = format!("**draft** e\u{301} 示例\n\n{}", "paragraph\n\n".repeat(50));
    state.apply_editor_command(EditorCommand::SelectAll, &PLATFORM);
    state.apply_editor_command(EditorCommand::InsertText(draft.clone()), &PLATFORM);
    for (offset, extend_selection) in [(2, false), (7, true)] {
        state.apply_editor_command(
            EditorCommand::MoveTo {
                position: EditorPosition::Source(offset),
                extend_selection,
            },
            &PLATFORM,
        );
    }
    state.app.dispatch_at(
        app::AppCommand::SetEditorViewport(EditorViewport {
            top_line: 20,
            left_column: 2,
        }),
        Instant::now(),
    );
    let source = state.app.editor_state().unwrap();
    let expected = (
        source.cursor,
        source.selection,
        source.viewport,
        source.revision(),
    );

    for key in [InputKey::F(6), InputKey::F(6), InputKey::Enter] {
        state.handle_editor_key(KeyInput::new(key), &PLATFORM);
    }
    assert_eq!(state.to_editor_view_model().markdown_reading, Some(true));
    assert_eq!(state.to_editor_view_model().mode, EditorMode::Source);
    assert_eq!(
        state.to_editor_view_model().scroll_line,
        expected.2.top_line
    );
    assert!(!state.editor_vim_available());
    poll_until(&mut state, |state| {
        state.editor_markdown_reading_cache().is_some()
    });

    let model = state.to_editor_view_model();
    let blocks = model.shared_blocks.unwrap();
    assert!(matches!(
        blocks.first(),
        Some(ui::EditorRenderBlock::Paragraph(spans)) if spans.iter().any(|span| span.text == "draft")
    ));
    let layout = state.editor_current_layout();
    let first_line = layout.rich_line_maps.first().unwrap();
    let y = layout.canvas.y + (first_line.document_line - layout.visible_start) as u16;
    for (x, kind) in [
        (
            layout.canvas.x,
            ui::MouseEventKind::Down(PointerButton::Left),
        ),
        (
            layout.canvas.x + 5,
            ui::MouseEventKind::Drag(PointerButton::Left),
        ),
        (
            layout.canvas.x + 5,
            ui::MouseEventKind::Up(PointerButton::Left),
        ),
    ] {
        mouse(&mut state, (x, y), kind);
    }
    state.apply_editor_command(EditorCommand::Copy, &PLATFORM);
    assert_eq!(state.text_clipboard().read().unwrap(), "draft");
    state.apply_editor_command(EditorCommand::InsertText("blocked".into()), &PLATFORM);
    state.handle_editor_key(KeyInput::new(InputKey::F(8)), &PLATFORM);
    assert!(state.app.editor_state().unwrap().is_markdown_reading());
    state.handle_editor_key(KeyInput::new(InputKey::End), &PLATFORM);
    assert!(state.app.editor_state().unwrap().viewport.top_line > 0);

    let toggle = state.editor_current_layout().markdown_reading_toggle;
    let point = (toggle.x, toggle.y);
    mouse(
        &mut state,
        point,
        ui::MouseEventKind::Down(PointerButton::Left),
    );
    assert!(state.app.editor_state().unwrap().is_markdown_reading());
    mouse(
        &mut state,
        point,
        ui::MouseEventKind::Up(PointerButton::Left),
    );
    let source = state.app.editor_state().unwrap();
    assert_eq!(source.mode, EditorMode::Source);
    assert_eq!(source.export_text(), draft);
    assert_eq!(
        (
            source.cursor,
            source.selection,
            source.viewport,
            source.revision()
        ),
        expected
    );
    assert!(source.is_dirty() && source.can_undo());

    state.toggle_editor_markdown_reading();
    assert!(!state.pages.editor.editor_markdown_reading.pending());
    assert!(Arc::ptr_eq(
        &blocks,
        &state.editor_markdown_reading_cache().unwrap().blocks,
    ));
}

#[test]
fn stale_markdown_reading_does_not_cross_edits_or_document_pages() {
    let mut state = session("stale.mdown", b"old draft");
    let owner = state.active_page_id();
    state.toggle_editor_markdown_reading();
    assert!(state.pages.editor.editor_markdown_reading.pending());
    state.toggle_editor_markdown_reading();
    state.apply_editor_command(EditorCommand::SelectAll, &PLATFORM);
    state.apply_editor_command(EditorCommand::InsertText("**new draft**".into()), &PLATFORM);
    state.toggle_editor_markdown_reading();

    state.new_edit_document();
    let covering = state.active_page_id();
    assert_ne!(covering, owner);
    poll_until(&mut state, |state| {
        !state
            .pages
            .editor
            .get(owner)
            .unwrap()
            .editor_markdown_reading
            .pending()
    });
    assert_eq!(state.active_page_id(), covering);
    assert!(state.to_editor_view_model().markdown_reading.is_none());
    state.activate_editor_document_page(owner);
    let model = state.to_editor_view_model();
    assert_eq!(model.markdown_reading, Some(true));
    assert!(matches!(
        model.render_blocks().first(),
        Some(ui::EditorRenderBlock::Paragraph(spans)) if spans.iter().any(|span| span.text == "new draft")
    ));
    state.toggle_editor_markdown_reading();
    state.apply_editor_command(EditorCommand::InsertText(" edit again".into()), &PLATFORM);
    state.toggle_editor_markdown_reading();
    state.close_page_instance(owner);
    state.poll_editor_background_tasks(&PLATFORM);
    assert!(state.pages.editor.get(owner).is_none());
    assert!(
        !state
            .app
            .editor_state_for_page(covering)
            .unwrap()
            .is_markdown_reading()
    );
}

#[test]
fn markdown_save_as_reloads_relative_pictures_from_the_new_directory() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let original = root.join("original");
    let destination = root.join("destination");
    std::fs::create_dir_all(&original).unwrap();
    std::fs::create_dir_all(&destination).unwrap();
    std::fs::write(
        original.join("plot.png"),
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/editor/red-blue.png"
        )),
    )
    .unwrap();
    std::fs::write(
        destination.join("plot.png"),
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../app/tests/fixtures/office_preview/first-animation-frame.png"
        )),
    )
    .unwrap();
    let source = b"![Results](plot.png)\n";
    let path = original.join("guide.md");
    std::fs::write(&path, source).unwrap();
    let mut state = session(path, source);
    state.toggle_editor_markdown_reading();
    state.apply_input_with_platform(
        InputEvent::Resize {
            width: 108,
            height: 20,
        },
        &PLATFORM,
    );
    poll_until(&mut state, |state| {
        state.editor_markdown_reading_cache().is_some()
    });
    let image_size = |state: &ShellSession| {
        state
            .editor_markdown_reading_cache()
            .unwrap()
            .blocks
            .iter()
            .find_map(|block| match block {
                ui::EditorRenderBlock::PreviewGraphic {
                    graphic,
                    kind: ui::EditorPreviewGraphicKind::Picture,
                    ..
                } => Some((graphic.image.width, graphic.image.height)),
                _ => None,
            })
    };
    assert!(state.pages.editor.editor_markdown_reading.pending());
    assert_eq!(image_size(&state), None);
    assert!(matches!(
        state.to_editor_view_model().read_only_layout,
        Some(ui::EditorReadOnlyLayoutState::Ready(_))
    ));
    assert!(state.editor_current_layout().document_line_count > 0);
    state.apply_input_with_platform(
        InputEvent::Resize {
            width: 180,
            height: 50,
        },
        &PLATFORM,
    );
    poll_until(&mut state, |state| {
        !state.pages.editor.editor_markdown_reading.pending()
    });
    assert_eq!(image_size(&state), Some((160, 192)));
    assert!(state.editor_current_layout().document_line_count > 0);
    state.app.dispatch_at(
        app::AppCommand::Editor(EditorCommand::RequestSaveAs),
        Instant::now(),
    );
    let Some(app::editor::EditorEffect::SaveFilePicker { snapshot, .. }) =
        state.app.take_editor_effects().pop()
    else {
        panic!("reading mode should allow Save As");
    };
    let saved = destination.join("guide.markdown");
    assert!(state.save_editor_document(saved.clone(), snapshot, &PLATFORM));
    poll_until(&mut state, |state| {
        state.pages.editor.editor_save_state.is_none()
            && !state.pages.editor.editor_markdown_reading.pending()
            && state.editor_markdown_reading_cache().is_some()
    });
    assert_eq!(
        state.app.editor_state().unwrap().document.path.as_ref(),
        Some(&saved)
    );
    assert_eq!(image_size(&state), Some((1, 1)));
    assert_eq!(std::fs::read(&saved).unwrap(), source);
}

#[test]
fn dirty_markdown_can_recover_and_save_on_close_from_reading() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path().canonicalize().unwrap();
    let path = root.join("draft.mkd");
    std::fs::write(&path, b"\xEF\xBB\xBForiginal\r\n").unwrap();
    let mut state = session(&path, &std::fs::read(&path).unwrap());
    let paths = platform::AppPaths::from_parts(
        root.join("config"),
        root.join("data"),
        root.join("cache"),
        root.join("logs"),
        root.join("temp"),
    )
    .unwrap();
    state.storage_manager = Some(storage::StorageManager::open(paths).unwrap().manager);
    state.app.dispatch_at(
        app::AppCommand::SetAuthSession(Some(AuthSession {
            source: identity::IdentitySource::SystemCurrentProcess,
            session_id: "markdown-recovery".into(),
            user_id: "alice".into(),
            username: "alice".into(),
            role: identity::UserRole::Admin,
            started_at_epoch_ms: 1,
        })),
        Instant::now(),
    );
    state.pages.editor.editor_fingerprint =
        Some(platform::read_document_bytes(&path).unwrap().fingerprint);
    state.apply_editor_command(EditorCommand::SelectAll, &PLATFORM);
    state.apply_editor_command(EditorCommand::InsertText("**draft**\r\n".into()), &PLATFORM);
    state.toggle_editor_markdown_reading();
    poll_until(&mut state, |state| {
        state.editor_markdown_reading_cache().is_some()
    });
    assert!(state.persist_editor_recovery_now(Instant::now()));
    let recovery = state
        .pages
        .editor
        .editor_recovery_lease
        .as_ref()
        .unwrap()
        .path()
        .to_owned();
    assert!(
        std::fs::read_to_string(&recovery)
            .unwrap()
            .contains("**draft**")
    );

    let owner = state.active_page_id();
    state.request_editor_close(&PLATFORM);
    assert!(state.notification_active_modal_id().is_some());
    state.apply_routed_event(
        RoutedEvent {
            input: InputEvent::Tick,
            target: RoutedTarget::Component(ShellComponent::Editor),
            command: ShellCommand::EditorSaveAndClose,
        },
        &PLATFORM,
        Instant::now(),
    );
    poll_until(&mut state, |state| state.page_screen(owner).is_none());
    assert_eq!(std::fs::read(&path).unwrap(), b"\xEF\xBB\xBF**draft**\r\n");
    assert!(!recovery.exists());
}
