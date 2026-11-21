use super::*;

fn open_explorer_blank_context_for_test(state: &mut ShellSession, platform: &dyn Platform) {
    let layout = explorer_test_layout(state);
    let point = (layout.table_body.x, layout.table_body.bottom() - 1);
    assert_eq!(
        layout.hit_test(point.0, point.1),
        Some(ui::ExplorerHitTarget::EmptyTable)
    );
    state.apply_input_with_platform(
        InputEvent::mouse_down(PointerButton::Right, point),
        platform,
    );
}

fn explorer_test_context_menu(state: &ShellSession) -> ui::ExplorerContextMenuViewModel {
    let Some(ui::ExplorerOverlayViewModel::ContextMenu(menu)) =
        state.to_explorer_view_model().overlay
    else {
        panic!("expected Explorer context menu");
    };
    menu
}

#[test]
fn trash_delete_never_empties_other_items_and_empty_confirmation_defaults_to_cancel() {
    let (mut state, platform, _guard) = explorer_terminal_test_fixture();
    state.storage_manager = Some(
        StorageManager::open(platform.app_paths().unwrap())
            .unwrap()
            .manager,
    );
    state.update_explorer_state(|explorer| {
        explorer.current_location = app::explorer::ExplorerLocation::Trash;
        explorer.select_index(0, app::explorer::ExplorerSelectionMode::Replace);
    });
    let before = state.app.explorer_state().unwrap().entries.clone();
    state.apply_input_with_platform(InputEvent::key(InputKey::Delete), &platform);
    assert!(state.to_notification_view_model().is_none());
    assert!(state.app.explorer_state().unwrap().pending_dialog.is_none());
    open_explorer_blank_context_for_test(&mut state, &platform);
    assert!(
        explorer_test_context_menu(&state)
            .items
            .iter()
            .any(|item| item.id == "dump-trash" && item.shortcut.is_none())
    );
    state.apply_input_with_platform(InputEvent::key(InputKey::Delete), &platform);
    assert!(state.app.explorer_state().unwrap().pending_dialog.is_none());
    state.close_explorer_popup();
    state.activate_explorer_toolbar(ui::ExplorerToolbarAction::DumpTrash, (0, 0), &platform);
    let modal = state.to_notification_view_model().unwrap();
    assert_eq!(
        modal
            .actions
            .iter()
            .find(|action| action.selected)
            .unwrap()
            .id,
        "cancel"
    );
    state.apply_input_with_platform(InputEvent::key(InputKey::Enter), &platform);
    assert!(state.to_notification_view_model().is_none());
    assert!(state.app.explorer_state().unwrap().pending_dialog.is_none());
    assert_eq!(state.app.explorer_state().unwrap().entries, before);
    assert!(
        !platform
            .calls()
            .contains(&platform::mock::MockCall::EmptyTrash)
    );
}

#[test]
fn explorer_never_receives_shell_chrome_pointer_commands_and_clears_drag() {
    let mut state = explorer_routing_test_state();
    let _ = state.update_explorer_state(|explorer| {
        explorer.drag = Some(app::explorer::ExplorerDragState {
            sources: vec![std::path::PathBuf::from("source")],
            target: None,
            mode: app::explorer::ExplorerTransferMode::Copy,
            active: true,
        });
    });
    let top = hit_region_center(&state, ShellComponent::TopBar);

    let routed = state.route_input_at(
        InputEvent::mouse_drag(PointerButton::Left, top),
        Instant::now(),
    );
    assert_eq!(
        routed.target,
        RoutedTarget::Component(ShellComponent::TopBar)
    );
    assert_eq!(routed.command, ShellCommand::CaptureOverlayInput);
    assert!(
        state
            .app
            .explorer_state()
            .expect("Explorer state")
            .drag
            .is_none()
    );

    let status = hit_region_center(&state, ShellComponent::StatusBar);
    let routed = state.route_input_at(
        InputEvent::mouse_down(PointerButton::Left, status),
        Instant::now(),
    );
    assert_eq!(
        routed.target,
        RoutedTarget::Component(ShellComponent::StatusBar)
    );
    assert_eq!(
        routed.command,
        ShellCommand::Activate {
            target: ShellComponent::StatusBar,
            coordinates: status,
            click: ClickKind::Single,
        }
    );
    for input in [
        InputEvent::mouse_down(PointerButton::Right, status),
        InputEvent::Mouse(ui::MouseEvent {
            position: ui::Point::new(status.0, status.1),
            kind: ui::MouseEventKind::Scroll(ScrollDirection::Down),
            modifiers: InputModifiers::none(),
        }),
    ] {
        let routed = state.route_input_at(input, Instant::now());
        assert_eq!(
            routed.target,
            RoutedTarget::Component(ShellComponent::StatusBar)
        );
        assert_eq!(routed.command, ShellCommand::CaptureOverlayInput);
    }

    let _ = state.update_explorer_state(|explorer| {
        explorer.drag = Some(app::explorer::ExplorerDragState {
            sources: vec![std::path::PathBuf::from("source")],
            target: None,
            mode: app::explorer::ExplorerTransferMode::Move,
            active: true,
        });
    });
    let (target, command) = state.route_explorer_mouse(
        ui::MouseEvent {
            position: ui::Point::new(top.0, top.1),
            kind: ui::MouseEventKind::Up(PointerButton::Left),
            modifiers: InputModifiers::none(),
        },
        Some(ShellComponent::TopBar),
        Instant::now(),
    );
    assert_eq!(target, RoutedTarget::Component(ShellComponent::TopBar));
    assert_eq!(command, ShellCommand::CaptureOverlayInput);
    assert!(
        state
            .app
            .explorer_state()
            .expect("Explorer state")
            .drag
            .is_none()
    );
}

#[test]
fn explorer_toolbar_shortcuts_route_without_shifted_symbols() {
    use ui::ExplorerToolbarAction as Action;
    let mut state = explorer_routing_test_state();
    for (action, command) in [
        (Action::Back, ShellCommand::ExplorerOpenBack),
        (Action::Forward, ShellCommand::ExplorerOpenForward),
        (Action::Up, ShellCommand::ExplorerOpenParent),
        (Action::New, ShellCommand::BeginExplorerNewFolder),
        (Action::Cut, ShellCommand::ExplorerCut),
        (Action::Copy, ShellCommand::ExplorerCopy),
        (Action::Paste, ShellCommand::ExplorerPaste),
        (Action::Rename, ShellCommand::BeginExplorerRename),
        (Action::Delete, ShellCommand::ExplorerDelete),
        (
            Action::Refresh,
            ShellCommand::ExplorerToolbarShortcut(Action::Refresh),
        ),
        (
            Action::Sort,
            ShellCommand::ExplorerToolbarShortcut(Action::Sort),
        ),
        (
            Action::Options,
            ShellCommand::ExplorerToolbarShortcut(Action::Options),
        ),
    ] {
        let label = action.shortcut_label();
        let key = if matches!(action, Action::Copy | Action::Cut | Action::Paste) {
            let character = match action {
                Action::Copy => 'c',
                Action::Cut => 'x',
                _ => 'v',
            };
            KeyInput::with_modifiers(
                InputKey::Char(character),
                if cfg!(target_os = "macos") {
                    InputModifiers {
                        super_key: true,
                        ..InputModifiers::NONE
                    }
                } else {
                    InputModifiers::CTRL
                },
            )
        } else if label == "Del" {
            KeyInput::new(InputKey::Delete)
        } else {
            KeyInput::from_label(label)
        };
        assert_eq!(state.route_key_input(&key).1, command, "{action:?}");
        if let InputKey::Char(character) = key.key {
            for (character, modifiers) in [
                (character.to_ascii_lowercase(), InputModifiers::NONE),
                (character, InputModifiers::SHIFT),
            ] {
                assert_eq!(
                    state
                        .route_key_input(&KeyInput::with_modifiers(
                            InputKey::Char(character),
                            modifiers
                        ))
                        .1,
                    command,
                    "{action:?}: {modifiers:?}"
                );
            }
        }
    }
    for (key, command) in [
        (InputKey::Left, ShellCommand::ExplorerOpenBack),
        (InputKey::Right, ShellCommand::ExplorerOpenForward),
    ] {
        assert_eq!(
            state
                .route_key_input(&KeyInput::with_modifiers(key, InputModifiers::ALT))
                .1,
            command
        );
    }

    state.begin_explorer_input(ExplorerInputMode::NewFolder);
    let key = KeyInput::from_label("N");
    assert_eq!(
        state.route_key_input(&key).1,
        ShellCommand::ExplorerEditInput(InputEvent::Key(key.clone()))
    );
    state.apply_input(InputEvent::Key(key));
    assert_eq!(state.pages.explorer.explorer_input, "N");
    for (label, expected, cursor) in [("Left", "N", 0), ("Delete", "", 0)] {
        let key = KeyInput::from_label(label);
        assert_eq!(
            state.route_key_input(&key).1,
            ShellCommand::ExplorerEditInput(InputEvent::Key(key.clone()))
        );
        state.apply_input(InputEvent::Key(key));
        assert_eq!(state.pages.explorer.explorer_input, expected);
        assert_eq!(state.pages.explorer.explorer_address_cursor, cursor);
        assert_eq!(state.pages.explorer.explorer_overlay_selection, 0);
        assert_eq!(
            state.pages.explorer.explorer_input_mode,
            ExplorerInputMode::NewFolder
        );
    }
}

#[test]
fn delayed_explorer_rename_and_undo_keep_new_search_sort_and_selection() {
    use app::explorer::{ExplorerSelectionMode, ExplorerSortDirection, ExplorerSortField};

    for (undo, query) in [(false, "second"), (true, "txt")] {
        let (mut state, platform, _guard) = explorer_terminal_test_fixture();
        let storage = StorageManager::open(platform.app_paths().unwrap())
            .unwrap()
            .manager;
        let controller = state.app.explorer_controller();
        let auth = state.app.auth_session().cloned();
        let mut submitted = state.app.explorer_state().unwrap().clone();
        submitted.select_index(0, ExplorerSelectionMode::Replace);
        let directory = submitted.current_path.clone();
        if undo {
            controller.apply(
                &mut submitted,
                ExplorerCommand::Rename("renamed.txt".into()),
                auth.as_ref(),
                &platform,
                &storage,
            );
            assert!(submitted.error.is_none());
        }
        state.replace_explorer_state(Some(submitted.clone()));

        // The worker retains the submission snapshot while the page accepts new input.
        for command in [
            ExplorerCommand::Search(query.into()),
            ExplorerCommand::SetSort(ExplorerSortField::Name),
            ExplorerCommand::SelectIndex(0),
        ] {
            state
                .app
                .dispatch_explorer_at(command, &platform, &storage, Instant::now());
        }
        assert_eq!(
            state.app.explorer_state().unwrap().selected_path(),
            Some(directory.join("second.txt"))
        );

        controller.apply(
            &mut submitted,
            if undo {
                ExplorerCommand::Undo
            } else {
                ExplorerCommand::Rename("renamed.txt".into())
            },
            auth.as_ref(),
            &platform,
            &storage,
        );
        assert!(submitted.error.is_none());
        state.complete_explorer_workspace_command(submitted);

        let live = state.app.explorer_state().unwrap();
        assert_eq!(live.query, query);
        assert_eq!(live.sort_field, ExplorerSortField::Name);
        assert_eq!(live.sort_direction, ExplorerSortDirection::Descending);
        assert_eq!(live.selected_path(), Some(directory.join("second.txt")));
        assert_eq!(
            live.effective_selected_paths(),
            [directory.join("second.txt")]
        );
        assert_eq!(live.entries[0].name, "second.txt");
        assert_eq!(live.entries.len(), if undo { 2 } else { 1 });
        assert_eq!(directory.join("first.txt").exists(), undo);
        assert_eq!(directory.join("renamed.txt").exists(), !undo);
        assert!(
            live.all_entries
                .iter()
                .any(|entry| { entry.name == if undo { "first.txt" } else { "renamed.txt" } })
        );
    }
}

#[test]
fn explorer_selection_taps_wait_for_release_and_cancel_when_a_form_opens() {
    for hover in [false, true] {
        let (mut state, platform, _guard) = explorer_terminal_test_fixture();
        state.storage_manager = Some(
            StorageManager::open(platform.app_paths().unwrap())
                .unwrap()
                .manager,
        );
        while state.notification_dismiss_active_modal_without_response() {}
        state
            .update_explorer_state(|explorer| explorer.clear_selection())
            .unwrap();
        state.activate_explorer_toolbar(
            ui::ExplorerToolbarAction::SelectionMode,
            (0, 0),
            &platform,
        );
        assert!(
            state
                .app
                .explorer_state()
                .unwrap()
                .effective_selected_paths()
                .is_empty()
        );
        let row = explorer_test_layout(&state).rows[0].area;
        let point = (row.x, row.y);
        assert!(state.explorer_selection_button_at(point).is_some());
        if hover {
            state.apply_input_with_platform(InputEvent::mouse_moved(point), &platform);
        }
        state.apply_input_with_platform(
            InputEvent::mouse_down(PointerButton::Left, point),
            &platform,
        );
        assert!(state.interaction.button_capture.is_some());
        assert!(
            state
                .app
                .explorer_state()
                .unwrap()
                .effective_selected_paths()
                .is_empty()
        );
        state
            .apply_input_with_platform(InputEvent::mouse_up(PointerButton::Left, point), &platform);
        assert_eq!(
            state
                .app
                .explorer_state()
                .unwrap()
                .effective_selected_paths()
                .len(),
            1
        );
        assert_eq!(state.active_screen(), ShellScreen::Explorer);
        state.apply_input_with_platform(
            InputEvent::mouse_down(PointerButton::Left, point),
            &platform,
        );
        state.begin_explorer_input(ExplorerInputMode::NewFolder);
        state
            .apply_input_with_platform(InputEvent::mouse_up(PointerButton::Left, point), &platform);
        assert_eq!(
            state
                .app
                .explorer_state()
                .unwrap()
                .effective_selected_paths()
                .len(),
            1
        );
        state.apply_input_with_platform(InputEvent::key(InputKey::Escape), &platform);
        state.activate_explorer_toolbar(
            ui::ExplorerToolbarAction::SelectionMode,
            (0, 0),
            &platform,
        );
        let explorer = state.app.explorer_state().unwrap();
        assert!(!explorer.selection_mode);
        assert!(explorer.selected_paths.is_empty());
        assert_eq!(explorer.effective_selected_paths().len(), 1);
    }
}

#[test]
fn explorer_blank_context_opens_terminal_at_directory_and_returns_without_reusing_it() {
    for (use_mouse, locale, label) in [
        (true, "en-US", "Open Terminal Here"),
        (false, "zh-CN", "在此处打开终端"),
    ] {
        let (mut state, platform, guard) = explorer_terminal_test_fixture();
        state.language = Arc::new(
            i18n::LanguageSnapshot::load(&guard.0, locale, 1)
                .unwrap()
                .snapshot,
        );
        state.refresh_hit_map();
        let selected = state
            .app
            .explorer_state()
            .unwrap()
            .effective_selected_paths();
        assert_eq!(selected.len(), 2);

        let row = explorer_test_layout(&state).rows[0].area;
        state.apply_input_with_platform(
            InputEvent::mouse_down(PointerButton::Right, (row.x, row.y)),
            &platform,
        );
        assert!(
            explorer_test_context_menu(&state)
                .items
                .iter()
                .all(|item| item.id != "open-terminal"),
            "a file selection has no directory terminal action"
        );
        state.apply_input_with_platform(InputEvent::from_key_label("Esc"), &platform);
        open_explorer_blank_context_for_test(&mut state, &platform);
        let explorer = state.app.explorer_state().unwrap().clone();
        assert!(explorer.effective_selected_paths().is_empty());
        let directory = explorer.current_path.clone();
        let menu = explorer_test_context_menu(&state);
        let index = menu
            .items
            .iter()
            .position(|item| item.id == "open-terminal")
            .unwrap();
        assert!(menu.items[index].enabled);
        assert_eq!(menu.items[index].label, label);

        if use_mouse {
            let control = explorer_test_layout(&state)
                .overlay
                .unwrap()
                .controls
                .into_iter()
                .find(|control| control.control == ui::ExplorerOverlayControl::ContextItem(index))
                .unwrap();
            let point = (control.area.x, control.area.y);
            state.apply_input_with_platform(
                InputEvent::mouse_down(PointerButton::Left, point),
                &platform,
            );
            state.apply_input_with_platform(
                InputEvent::mouse_up(PointerButton::Left, point),
                &platform,
            );
        } else {
            for _ in 0..index {
                state.apply_input_with_platform(InputEvent::from_key_label("Down"), &platform);
            }
            assert_eq!(
                explorer_test_context_menu(&state).selected_index,
                Some(index)
            );
            state.apply_input_with_platform(InputEvent::from_key_label("Enter"), &platform);
        }

        assert_eq!(state.active_screen(), ShellScreen::CommandLine);
        assert_eq!(state.focused_component(), ShellComponent::CommandLine);
        assert_eq!(
            state
                .pages
                .command_line
                .command_line_start_directory
                .as_ref(),
            Some(&directory)
        );
        assert!(state.pages.common.active_popup.is_none());
        assert!(state.pages.explorer.explorer_overlay_mode.is_none());
        assert_eq!(state.app.explorer_state(), Some(&explorer));
        state.close_command_line();
        assert_eq!(state.active_screen(), ShellScreen::Explorer);
        assert_eq!(state.focused_component(), ShellComponent::Explorer);
        assert!(
            state
                .pages
                .command_line
                .command_line_start_directory
                .is_none()
        );
        assert_eq!(state.app.explorer_state(), Some(&explorer));

        state.enter_screen(ShellScreen::Launcher);
        state.pages.common.focused_component = ShellComponent::Launcher;
        state.pages.launcher.launcher_selected_index = state
            .available_built_in_applications()
            .iter()
            .position(|application| application.id == app::COMMAND_LINE_APPLICATION.id)
            .unwrap();
        state.refresh_hit_map();
        state.apply_input_with_platform(InputEvent::from_key_label("Enter"), &platform);
        assert_eq!(state.active_screen(), ShellScreen::CommandLine);
        assert!(
            state
                .pages
                .command_line
                .command_line_start_directory
                .is_none()
        );
        state.close_command_line();
        assert_eq!(state.active_screen(), ShellScreen::Launcher);
    }
}

#[test]
fn explorer_terminal_action_cannot_bypass_permissions_size_or_trash_location() {
    let (mut state, platform, _guard) = explorer_terminal_test_fixture();
    set_test_auth_role(&mut state, UserRole::Guest);
    assert!(!state.can_execute_command_line());
    open_explorer_blank_context_for_test(&mut state, &platform);
    let menu = explorer_test_context_menu(&state);
    let index = menu
        .items
        .iter()
        .position(|item| item.id == "open-terminal")
        .unwrap();
    assert!(!menu.items[index].enabled);
    let control = explorer_test_layout(&state)
        .overlay
        .unwrap()
        .controls
        .into_iter()
        .find(|control| control.control == ui::ExplorerOverlayControl::ContextItem(index))
        .unwrap();
    assert!(!control.enabled);
    let point = (control.area.x, control.area.y);
    state.apply_input_with_platform(
        InputEvent::mouse_down(PointerButton::Left, point),
        &platform,
    );
    state.apply_input_with_platform(InputEvent::mouse_up(PointerButton::Left, point), &platform);
    for _ in 0..index {
        state.apply_input_with_platform(InputEvent::from_key_label("Down"), &platform);
    }
    assert_eq!(
        explorer_test_context_menu(&state).selected_index,
        Some(index)
    );
    state.apply_input_with_platform(InputEvent::from_key_label("Enter"), &platform);
    state.activate_explorer_overlay_control(
        ui::ExplorerOverlayControl::ContextItem(index),
        &platform,
    );
    assert_eq!(state.active_screen(), ShellScreen::Explorer);
    assert!(
        state
            .pages
            .command_line
            .command_line_start_directory
            .is_none()
    );
    assert!(state.pages.common.active_popup.is_some());
    let directory = state.app.explorer_state().unwrap().current_path.clone();
    state.open_command_line_at(Some(directory.clone()));
    assert_eq!(state.active_screen(), ShellScreen::Explorer);
    assert!(
        state
            .pages
            .command_line
            .command_line_start_directory
            .is_none()
    );
    while state.notification_dismiss_active_modal_without_response() {}
    state.close_explorer_popup();

    set_test_auth_role(&mut state, UserRole::Admin);
    state.terminal_size = (107, 22);
    state.refresh_hit_map();
    open_explorer_blank_context_for_test(&mut state, &platform);
    let index = explorer_test_context_menu(&state)
        .items
        .iter()
        .position(|item| item.id == "open-terminal")
        .unwrap();
    for _ in 0..index {
        state.apply_input_with_platform(InputEvent::from_key_label("Down"), &platform);
    }
    state.apply_input_with_platform(InputEvent::from_key_label("Enter"), &platform);
    assert_eq!(state.active_screen(), ShellScreen::Explorer);
    assert!(
        state
            .pages
            .command_line
            .command_line_start_directory
            .is_none()
    );
    assert!(state.pages.common.active_popup.is_none());
    while state.notification_dismiss_active_modal_without_response() {}
    state.terminal_size = (120, 40);
    state.update_explorer_state(|explorer| {
        explorer.current_location = app::explorer::ExplorerLocation::Trash;
        explorer.entries.clear();
        explorer.all_entries.clear();
        explorer.clear_selection();
    });
    state.refresh_hit_map();
    open_explorer_blank_context_for_test(&mut state, &platform);
    assert!(
        explorer_test_context_menu(&state)
            .items
            .iter()
            .all(|item| item.id != "open-terminal")
    );
    assert_eq!(state.active_screen(), ShellScreen::Explorer);
    assert!(
        state
            .pages
            .command_line
            .command_line_start_directory
            .is_none()
    );
}

#[test]
fn explorer_search_shortcut_preserves_text_input_and_sort() {
    let state = explorer_routing_test_state();
    for (character, modifiers) in [
        ('d', InputModifiers::ALT),
        ('d', InputModifiers::CTRL),
        ('s', InputModifiers::ALT),
        ('n', InputModifiers::ALT),
        ('r', InputModifiers::ALT),
        ('c', InputModifiers::ALT),
        ('x', InputModifiers::ALT),
        ('v', InputModifiers::ALT),
    ] {
        assert_eq!(
            state
                .route_key_input(&KeyInput::with_modifiers(
                    InputKey::Char(character),
                    modifiers
                ))
                .1,
            ShellCommand::RecordInput,
            "{character}: {modifiers:?}"
        );
    }
    for label in ["s", "S", "/"] {
        assert_eq!(
            state.route_key_input(&KeyInput::from_label(label)).1,
            ShellCommand::BeginExplorerSearch
        );
    }
    assert_eq!(
        state
            .route_key_input(&KeyInput::with_modifiers(
                InputKey::Char('f'),
                InputModifiers::CTRL
            ))
            .1,
        ShellCommand::BeginExplorerSearch
    );
    assert_eq!(
        state.route_key_input(&KeyInput::from_label("F6")).1,
        ShellCommand::ExplorerToolbarShortcut(ui::ExplorerToolbarAction::Sort)
    );
    let (mut state, platform, _fixture) = explorer_terminal_test_fixture();
    state.storage_manager = Some(
        StorageManager::open(platform.app_paths().unwrap())
            .unwrap()
            .manager,
    );
    for mode in [
        ExplorerInputMode::Address,
        ExplorerInputMode::Search,
        ExplorerInputMode::NewFolder,
        ExplorerInputMode::Rename,
    ] {
        state.begin_explorer_input(mode);
        state.pages.explorer.explorer_input.clear();
        state.pages.explorer.explorer_address_cursor = 0;
        state.pages.explorer.explorer_address_selection_anchor = None;
        let mut expected = String::new();
        for character in ['s', 'S', 'r', 'c', 'x', 'v', 'd'] {
            let key = KeyInput::new(InputKey::Char(character));
            assert_eq!(
                state.route_key_input(&key).1,
                ShellCommand::ExplorerEditInput(InputEvent::Key(key.clone())),
                "{mode:?}: {character}"
            );
            state.apply_input_with_platform(InputEvent::Key(key), &platform);
            expected.push(character);
            assert_eq!(
                state.pages.explorer.explorer_input, expected,
                "{mode:?}: {character}"
            );
            assert_eq!(state.pages.explorer.explorer_input_mode, mode);
            if mode == ExplorerInputMode::Search {
                assert_eq!(state.app.explorer_state().unwrap().query, expected);
            }
        }
        state.apply_input_with_platform(InputEvent::from_key_label("Esc"), &platform);
    }
}

#[test]
fn holding_explorer_search_key_does_not_type_into_the_new_search() {
    let mut state = explorer_routing_test_state();
    state.apply_input(InputEvent::from_key_label("s"));
    assert_eq!(
        state.pages.explorer.explorer_input_mode,
        ExplorerInputMode::Search
    );
    let repeated = KeyInput::with_phase(
        InputKey::Char('s'),
        InputModifiers::NONE,
        InputPhase::Repeat,
    );
    for _ in 0..3 {
        assert!(
            state
                .prepare_button_input(InputEvent::Key(repeated.clone()), Instant::now())
                .is_none()
        );
    }
    let press = InputEvent::from_key_label("s");
    assert!(state.prepare_button_input(press, Instant::now()).is_some());
    assert!(
        state
            .prepare_button_input(InputEvent::Key(repeated), Instant::now())
            .is_some()
    );
    state.pages.explorer.explorer_input_mode = ExplorerInputMode::Browse;
    assert_ne!(
        state
            .route_key_input(&KeyInput::with_phase(
                InputKey::F(6),
                InputModifiers::NONE,
                InputPhase::Repeat
            ))
            .1,
        ShellCommand::ExplorerToolbarShortcut(ui::ExplorerToolbarAction::Sort)
    );
}
