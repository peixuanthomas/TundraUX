use super::*;

const SAMPLE: &[u8] = include_bytes!("../../../../../app/tests/fixtures/pdf/preview.pdf");

fn pdf_fixture(
    bytes: &[u8],
) -> (
    ShellSession,
    platform::mock::MockPlatform,
    SystemStatusTempGuard,
    PathBuf,
) {
    let (mut state, platform, guard) = explorer_terminal_test_fixture();
    state.storage_manager = Some(
        StorageManager::open(platform.app_paths().unwrap())
            .unwrap()
            .manager,
    );
    let directory = guard.0.join("Documents");
    std::fs::create_dir_all(&directory).unwrap();
    let name = "预览 e\u{301}.PdF";
    let path = directory.join(name);
    std::fs::write(&path, bytes).unwrap();
    let mut attributes = state.app.explorer_state().unwrap().entries[0]
        .attributes
        .clone();
    attributes.path = path.clone();
    attributes.len = bytes.len() as u64;
    platform.set_directory_listing(
        directory.clone(),
        platform::DirectoryListing {
            path: directory,
            entries: vec![platform::DirectoryEntryMetadata {
                path: path.clone(),
                name: name.into(),
                attributes: Some(attributes),
                open_policy: platform::FileOpenPolicy::SystemDefault,
            }],
            warnings: Vec::new(),
        },
    );
    let mut config = state.app.storage_config().clone();
    config.editor.explorer_open_extensions = vec!["txt".into()];
    state.replace_storage_config(config);
    state.set_navigation_path(vec![ShellScreen::Home]);
    (state, platform, guard, path)
}

fn assert_no_system_open(platform: &platform::mock::MockPlatform) {
    use platform::mock::MockCall;
    let calls = platform.calls();
    let system_opens = calls
        .iter()
        .filter(|call| {
            matches!(
                call,
                MockCall::OpenPath(_) | MockCall::OpenWith { .. } | MockCall::OpenUri(_)
            )
        })
        .count();
    assert_eq!(system_opens, 0);
}

fn wait_for_pdf(
    state: &mut ShellSession,
    platform: &dyn Platform,
    page: usize,
) -> (u64, app::editor::PdfPageImage) {
    let timeout = Instant::now() + Duration::from_secs(5);
    loop {
        state.poll_editor_background_tasks(platform);
        if let Some(pdf) = state.to_editor_view_model().pdf {
            assert_eq!(pdf.page, page);
            assert_eq!(pdf.total, 2);
            assert!(pdf.error.is_none(), "{:?}", pdf.error);
            if let Some((id, image)) = pdf.image {
                return (id, image.raster);
            }
        }
        assert!(Instant::now() < timeout, "PDF page did not render");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn pdf_opens_in_the_workspace_from_explorer_and_editor_open() {
    for (picker, double_click) in [(false, false), (false, true), (true, false)] {
        let (mut state, platform, _guard, path) = pdf_fixture(SAMPLE);
        if picker {
            state.open_editor();
            state.apply_input_with_platform(
                InputEvent::Key(KeyInput::with_modifiers(
                    InputKey::Char('o'),
                    InputModifiers::CTRL,
                )),
                &platform,
            );
            assert!(matches!(
                state.pages.explorer.explorer_purpose,
                ExplorerPurpose::EditorOpen
            ));
        } else {
            state.open_explorer(&platform);
        }
        state.set_editor_vim_enabled(true);
        assert_eq!(state.active_screen(), ShellScreen::Explorer);
        assert!(state.select_explorer_path(&path));
        if double_click {
            let area = explorer_test_layout(&state).rows[0].area;
            let point = (area.x, area.y);
            for _ in 0..2 {
                state.apply_input_with_platform(
                    InputEvent::mouse_down(PointerButton::Left, point),
                    &platform,
                );
                state.apply_input_with_platform(
                    InputEvent::mouse_up(PointerButton::Left, point),
                    &platform,
                );
            }
        } else {
            state.apply_input_with_platform(InputEvent::key(InputKey::Enter), &platform);
        }
        wait_for_pdf(&mut state, &platform, 0);
        assert_eq!(state.active_screen(), ShellScreen::Editor);
        assert_eq!(
            state.app.editor_state().unwrap().document.path.as_deref(),
            Some(path.as_path())
        );
        assert!(state.app.editor_state().unwrap().is_read_only());
        assert!(!state.editor_vim_enabled());
        assert_no_system_open(&platform);
    }
}

#[test]
fn pdf_preview_keeps_the_latest_page_and_size_and_remains_read_only() {
    let (mut state, platform, _guard, path) = pdf_fixture(SAMPLE);
    assert!(state.open_editor_path(path.clone()));
    let (first, _) = wait_for_pdf(&mut state, &platform, 0);
    let point = {
        let layout = state.editor_current_layout();
        let footer = layout.status_bar;
        let pdf = layout.pdf.unwrap();
        (footer.x..footer.right())
            .map(|x| (x, footer.y))
            .find(|&point| {
                pdf.button_at(point)
                    .is_some_and(|button| button.id.as_str() == "editor.pdf.next")
            })
            .unwrap()
    };
    state.apply_input_with_platform(
        InputEvent::mouse_down(PointerButton::Left, point),
        &platform,
    );
    assert_eq!(state.to_editor_view_model().pdf.unwrap().page, 0);
    let outside = (point.0, point.1 - 1);
    state.apply_input_with_platform(
        InputEvent::mouse_drag(PointerButton::Left, outside),
        &platform,
    );
    state.apply_input_with_platform(
        InputEvent::mouse_up(PointerButton::Left, outside),
        &platform,
    );
    assert_eq!(state.to_editor_view_model().pdf.unwrap().page, 0);
    state.apply_input_with_platform(
        InputEvent::mouse_down(PointerButton::Left, point),
        &platform,
    );
    state.apply_input_with_platform(InputEvent::mouse_up(PointerButton::Left, point), &platform);
    assert_eq!(state.to_editor_view_model().pdf.unwrap().page, 1);
    state.poll_editor_background_tasks(&platform);
    state.apply_input_with_platform(
        InputEvent::Resize {
            width: 108,
            height: 20,
        },
        &platform,
    );
    state.apply_input_with_platform(InputEvent::key(InputKey::PageUp), &platform);
    state.poll_editor_background_tasks(&platform);
    state.apply_input_with_platform(InputEvent::key(InputKey::PageDown), &platform);
    assert!(state.to_editor_view_model().pdf.unwrap().image.is_none());
    let (latest, scan) = wait_for_pdf(&mut state, &platform, 1);
    assert_ne!(first, latest);
    let canvas = state.editor_current_layout().canvas;
    assert_eq!(scan.viewport, [canvas.width, canvas.height]);
    assert!(
        scan.rgba
            .chunks_exact(4)
            .any(|pixel| pixel == [162, 40, 20, 255])
    );

    for event in [
        InputEvent::key(InputKey::F(8)),
        InputEvent::key(InputKey::Char('x')),
        InputEvent::Paste("do not replace this file".into()),
        InputEvent::Key(KeyInput::with_modifiers(
            InputKey::Char('s'),
            InputModifiers::CTRL,
        )),
    ] {
        state.apply_input_with_platform(event, &platform);
    }
    assert!(!state.editor_vim_enabled());
    assert!(!state.app.editor_state().unwrap().is_dirty());
    assert_eq!(std::fs::read(&path).unwrap(), SAMPLE);
    assert!(state.pages.editor.editor_save_state.is_none());
}

#[test]
fn pdf_preview_jumps_to_a_valid_page_and_pans_without_rerendering() {
    let (mut state, platform, _guard, path) = pdf_fixture(SAMPLE);
    assert!(state.open_editor_path(path));
    wait_for_pdf(&mut state, &platform, 0);
    let layout = state.editor_current_layout();
    let footer = layout.status_bar;
    let controls = layout.pdf.unwrap();
    let point = (footer.x..footer.right())
        .map(|x| (x, footer.y))
        .find(|&point| {
            controls
                .button_at(point)
                .is_some_and(|button| button.id.as_str() == "editor.pdf.jump")
        })
        .unwrap();
    state.apply_input_with_platform(
        InputEvent::mouse_down(PointerButton::Left, point),
        &platform,
    );
    assert!(state.to_editor_view_model().pdf.unwrap().jump.is_none());
    state.apply_input_with_platform(InputEvent::mouse_up(PointerButton::Left, point), &platform);
    state.apply_input_with_platform(InputEvent::Paste("0".into()), &platform);
    state.apply_input_with_platform(InputEvent::key(InputKey::Enter), &platform);
    let pdf = state.to_editor_view_model().pdf.unwrap();
    assert_eq!(pdf.page, 0);
    assert_eq!(
        pdf.jump.unwrap().message,
        i18n::tr!("ui-editor-pdf-page-invalid", total = 2)
    );
    state.apply_input_with_platform(InputEvent::key(InputKey::Escape), &platform);
    assert!(state.to_editor_view_model().pdf.unwrap().jump.is_none());

    state.apply_input_with_platform(
        InputEvent::Key(KeyInput::with_modifiers(
            InputKey::Char('g'),
            InputModifiers::CTRL,
        )),
        &platform,
    );
    state.apply_input_with_platform(InputEvent::Paste("2".into()), &platform);
    state.apply_input_with_platform(InputEvent::key(InputKey::Enter), &platform);
    let (_, fitted) = wait_for_pdf(&mut state, &platform, 1);
    assert!(state.to_editor_view_model().pdf.unwrap().jump.is_none());

    for key in [InputKey::Char('r'), InputKey::Char('+')] {
        state.apply_input_with_platform(InputEvent::key(key), &platform);
    }
    let (id, raster) = wait_for_pdf(&mut state, &platform, 1);
    assert_ne!(fitted.cells, raster.cells);
    let canvas = state.editor_current_layout().canvas;
    let size = raster.cells.map(u32::from);
    let zoom = state.to_editor_view_model().pdf.unwrap().zoom;
    assert_eq!(zoom.percent(), 150);
    let center = zoom.window(canvas, size);
    assert_eq!(center.offset[1], center.limit[1] / 2);
    state.apply_input_with_platform(InputEvent::key(InputKey::Down), &platform);
    let pdf = state.to_editor_view_model().pdf.unwrap();
    assert_eq!(
        pdf.zoom.window(canvas, size).offset[1],
        center.offset[1] + 1
    );
    assert_eq!(pdf.image.unwrap().0, id);
}

#[test]
fn pdf_preview_reuses_prefetched_pages_until_pixel_cell_size_changes() {
    let (mut state, platform, _guard, path) = pdf_fixture(SAMPLE);
    assert!(state.open_editor_path(path));
    let (first, vector) = wait_for_pdf(&mut state, &platform, 0);
    let deadline = Instant::now() + Duration::from_secs(5);
    while state.editor_pdf_has_background_work() {
        state.poll_editor_background_tasks(&platform);
        assert!(Instant::now() < deadline, "PDF prefetch did not settle");
        std::thread::sleep(Duration::from_millis(5));
    }

    state.apply_input_with_platform(InputEvent::key(InputKey::PageDown), &platform);
    let (second, scan) = state
        .to_editor_view_model()
        .pdf
        .unwrap()
        .image
        .expect("second page must be ready without another background poll");
    assert_ne!(second, first);
    assert_eq!(scan.raster.viewport, vector.viewport);
    assert!(
        scan.raster
            .rgba
            .chunks_exact(4)
            .any(|pixel| pixel == [162, 40, 20, 255])
    );

    state.apply_input_with_platform(InputEvent::key(InputKey::PageUp), &platform);
    let (again, cached) = state.to_editor_view_model().pdf.unwrap().image.unwrap();
    assert_eq!(again, first);
    assert!(Arc::ptr_eq(&cached.raster.rgba, &vector.rgba));

    state.terminal_image_cell_size = [12, 25];
    assert!(state.to_editor_view_model().pdf.unwrap().image.is_none());
    let (resized, raster) = wait_for_pdf(&mut state, &platform, 0);
    assert_ne!(resized, first);
    assert_eq!(raster.viewport, vector.viewport);
    assert_eq!(raster.cell_size, [12, 25]);
    assert_ne!(raster.width, vector.width);
}

#[test]
fn failed_pdf_preview_returns_to_explorer_and_reports_the_error_locally() {
    let protected = include_bytes!("../../../../../app/tests/fixtures/pdf/password.pdf");
    for (bytes, reason) in [
        (
            b"%PDF-1.4\nbroken".as_slice(),
            i18n::tr!("editor-pdf-invalid"),
        ),
        (protected.as_slice(), i18n::tr!("editor-pdf-encrypted")),
    ] {
        let (mut state, platform, _guard, path) = pdf_fixture(bytes);
        state.open_explorer(&platform);
        let explorer = state.active_page_id();
        assert!(state.select_explorer_path(&path));
        state.apply_input_with_platform(InputEvent::key(InputKey::Enter), &platform);
        let timeout = Instant::now() + Duration::from_secs(5);
        while state.active_page_id() != explorer {
            state.poll_editor_background_tasks(&platform);
            assert!(
                Instant::now() < timeout,
                "bad PDF did not return to Explorer"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        let error = state
            .pages
            .common
            .error_message
            .as_ref()
            .unwrap()
            .render_current();
        assert!(error.contains(&reason));
        assert_no_system_open(&platform);
    }
}
