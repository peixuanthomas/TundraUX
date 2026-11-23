use super::*;

fn state() -> ShellSession {
    ShellSession::new_for_home_mode(ShellLaunchConfig::default(), (120, 40), ShellHomeMode::User)
}

fn back(state: &mut ShellSession, pointer: bool) {
    if pointer {
        state.refresh_hit_map();
        let area = state
            .hit_map()
            .regions()
            .iter()
            .find(|region| region.component == ShellComponent::BackButton)
            .unwrap()
            .area;
        let point = (area.x + area.width / 2, area.y + area.height / 2);
        state.apply_input(InputEvent::mouse_down(PointerButton::Left, point));
        state.apply_input(InputEvent::mouse_up(PointerButton::Left, point));
    } else {
        state.apply_input(InputEvent::from_key_label("Esc"));
    }
}

fn prepare_page(state: &mut ShellSession, screen: ShellScreen) {
    state.enter_screen(screen);
    if screen == ShellScreen::Editor {
        state.app.dispatch_at(
            app::AppCommand::SetEditorState(Some(EditorState::new())),
            Instant::now(),
        );
    }
    if screen == ShellScreen::Settings {
        state.pages.settings.settings_state = Some(SettingsState {
            focus: Default::default(),
            form_selected: 0,
            form_interaction: Default::default(),
            application: Default::default(),
            system_time: Default::default(),
            animation_editor: None,
            number_editor: None,
            category: ui::SettingsCategory::Appearance,
            selected_field: ui::SettingsField::Theme,
            status: i18n::msg!("settings-ready").into(),
            scroll_offset: 0,
            picker: None,
            color_editor: None,
            weather_location_editor: None,
            file_extensions_editor: None,
            time_sync_server_editor: None,
            time_sync_validation_request_id: None,
        });
    }
    state.refresh_hit_map();
}

#[test]
fn every_page_returns_to_actual_caller_for_escape_and_back_button() {
    for parent in [ShellScreen::Clock, ShellScreen::Explorer, ShellScreen::Logs] {
        for page in [
            ShellScreen::Clock,
            ShellScreen::Diagnostics,
            ShellScreen::Logs,
            ShellScreen::Management,
            ShellScreen::SystemStatus,
            ShellScreen::Explorer,
            ShellScreen::Launcher,
            ShellScreen::Editor,
            ShellScreen::Settings,
        ] {
            if parent == page {
                continue;
            }
            for pointer in [false, true] {
                let mut state = state();
                state.enter_screen(parent);
                prepare_page(&mut state, page);
                back(&mut state, pointer);
                assert_eq!(
                    state.screen_stack(),
                    &[ShellScreen::Home, parent],
                    "{parent:?} -> {page:?}, pointer={pointer}"
                );
                assert_eq!(state.pages.common.focused_component, screen_focus(parent));
            }
        }
    }
}

#[test]
fn reset_discards_old_paths_and_wrong_page_close_is_ignored() {
    let mut state = state();
    state.enter_screen(ShellScreen::Launcher);
    state.enter_screen(ShellScreen::Editor);
    state.return_from_screen(ShellScreen::Launcher);
    assert_eq!(state.active_screen(), ShellScreen::Editor);
    state.enter_screen(ShellScreen::Editor);
    assert_eq!(state.screen_stack().len(), 3);
    state.reset_navigation(ShellScreen::FirstRunSetup);
    state.enter_screen(ShellScreen::ExitConfirm);
    state.cancel_exit_confirmation();
    assert_eq!(state.screen_stack(), &[ShellScreen::FirstRunSetup]);
    state.reset_navigation(ShellScreen::Home);
    state.enter_screen(ShellScreen::Launcher);
    state.close_launcher();
    assert_eq!(state.screen_stack(), &[ShellScreen::Home]);
}

#[test]
fn nested_visits_restore_each_callers_focus_and_ignore_held_escape() {
    let mut state = state();
    state.pages.common.focused_component = ShellComponent::StatusBar;
    state.open_clock();
    state.pages.common.focused_component = ShellComponent::ClockNewButton;
    prepare_page(&mut state, ShellScreen::Settings);
    back(&mut state, false);
    assert_eq!(state.active_screen(), ShellScreen::Clock);
    assert_eq!(
        state.pages.common.focused_component,
        ShellComponent::ClockNewButton
    );
    for phase in [InputPhase::Repeat, InputPhase::Release] {
        state.apply_input(InputEvent::Key(KeyInput::with_phase(
            InputKey::Escape,
            InputModifiers::NONE,
            phase,
        )));
        assert_eq!(state.active_screen(), ShellScreen::Clock);
    }
    assert!(state.header_back_label().is_none());
    back(&mut state, false);
    assert_eq!(state.active_screen(), ShellScreen::Clock);
    state.close_clock();
    assert_eq!(state.active_screen(), ShellScreen::Home);
    assert_eq!(
        state.pages.common.focused_component,
        ShellComponent::StatusBar
    );
}

#[test]
fn dirty_editor_back_requests_confirmation_and_cancel_keeps_path() {
    for pointer in [false, true] {
        let mut state = state();
        state.enter_screen(ShellScreen::Logs);
        prepare_page(&mut state, ShellScreen::Editor);
        state.apply_input(InputEvent::from_key_label("x"));
        assert!(state.app.editor_state().unwrap().is_dirty());
        back(&mut state, pointer);
        assert!(state.notification_has_active_modal());
        assert_eq!(state.active_screen(), ShellScreen::Editor);
        back(&mut state, pointer);
        assert!(!state.notification_has_active_modal());
        assert!(state.app.editor_state().unwrap().is_dirty());
        assert_eq!(
            state.screen_stack(),
            &[ShellScreen::Home, ShellScreen::Logs, ShellScreen::Editor]
        );
    }
}

#[test]
fn editor_rollback_restores_picker_beneath_overlay_and_rejects_stale_results() {
    let mut state = state();
    state.enter_screen(ShellScreen::Launcher);
    state.enter_screen(ShellScreen::Editor);
    state.enter_screen(ShellScreen::Explorer);
    let rollback = state.begin_editor_navigation(true);
    state.open_clock();
    let operation = EditorLoadOperation::Open {
        navigation: EditorLoadNavigation::EditorPicker,
        rollback,
        reload: None,
        replacing_dirty: false,
    };
    state.restore_editor_load_navigation(&operation);
    state.restore_editor_load_navigation(&operation);
    assert_eq!(
        state.screen_stack(),
        &[
            ShellScreen::Home,
            ShellScreen::Launcher,
            ShellScreen::Editor,
            ShellScreen::Explorer,
            ShellScreen::Clock
        ]
    );
    state.close_clock();
    assert_eq!(state.active_screen(), ShellScreen::Explorer);
    assert_eq!(
        state.pages.common.focused_component,
        ShellComponent::Explorer
    );

    state.return_from_screen(ShellScreen::Explorer);
    state.return_from_screen(ShellScreen::Editor);
    let rollback = state.begin_editor_navigation(false);
    state.return_from_screen(ShellScreen::Editor);
    state.enter_screen(ShellScreen::Editor);
    state.restore_editor_load_navigation(&EditorLoadOperation::Open {
        navigation: EditorLoadNavigation::Editor,
        rollback,
        reload: None,
        replacing_dirty: false,
    });
    assert_eq!(state.active_screen(), ShellScreen::Editor);
}

#[test]
fn command_line_terminal_keys_are_forwarded_without_a_back_button() {
    let mut state = state();
    state.enter_screen(ShellScreen::Explorer);
    state.enter_screen(ShellScreen::CommandLine);
    state.refresh_hit_map();
    let escape = InputEvent::from_key_label("Esc");
    assert_eq!(
        state.normalize_shell_navigation_input(escape.clone()),
        escape
    );
    for key in [
        KeyInput::from_label("Esc"),
        KeyInput::with_modifiers(InputKey::Char('x'), InputModifiers::CTRL_SHIFT),
    ] {
        let (_, command) = state.route_key_input(&key);
        assert_eq!(command, ShellCommand::CommandLineKey(key));
    }
    assert!(state.frame_layout.unwrap().back_button.is_none());
    state.apply_input(escape);
    assert!(state.command_line_close_request.is_none());
    assert_eq!(state.active_screen(), ShellScreen::CommandLine);
    state.close_command_line();
    assert_eq!(state.active_screen(), ShellScreen::Explorer);
}

#[test]
fn tool_roots_keep_the_same_page_open_with_escape() {
    for screen in [
        ShellScreen::Clock,
        ShellScreen::Diagnostics,
        ShellScreen::Editor,
        ShellScreen::Explorer,
        ShellScreen::Logs,
        ShellScreen::Management,
        ShellScreen::Settings,
        ShellScreen::SystemStatus,
    ] {
        for caller in [ShellScreen::Home, ShellScreen::Launcher] {
            let mut state = state();
            state.enter_screen(caller);
            prepare_page(&mut state, screen);
            let page = state.active_page_id();
            assert!(
                state.frame_layout.unwrap().back_button.is_none(),
                "{screen:?}"
            );
            back(&mut state, false);
            assert_eq!(state.active_page_id(), page);
            assert!(!state.notification_has_active_modal());
        }
    }
}

#[test]
fn power_title_supports_keyboard_and_keeps_a_covered_popup_after_cancel() {
    let mut state = state();
    while state.notification_dismiss_active_modal_without_response() {}
    state.refresh_hit_map();
    state.apply_input(InputEvent::from_key_label("Shift+Tab"));
    assert_eq!(state.focused_component(), ShellComponent::TopBar);
    state.apply_input(InputEvent::key(InputKey::Enter));
    assert_eq!(state.active_screen(), ShellScreen::ExitConfirm);
    let menu = state.to_notification_view_model().unwrap();
    assert!(
        menu.actions
            .iter()
            .any(|action| action.id == "cancel" && action.selected)
    );
    back(&mut state, false);
    assert_eq!(state.focused_component(), ShellComponent::TopBar);

    state.enter_screen(ShellScreen::Launcher);
    let launcher = state.active_page_id();
    let popup = ShellPopup {
        owner: None,
        anchor: (5, 5),
    };
    state.pages.common.active_popup = Some(popup);
    state.refresh_hit_map();
    let title = state.frame_layout.unwrap().title;
    let point = (title.x + 1, title.y);
    state.apply_input(InputEvent::mouse_down(PointerButton::Left, point));
    assert_eq!(state.active_page_id(), launcher);
    state.apply_input(InputEvent::mouse_up(PointerButton::Left, point));
    assert_eq!(state.active_screen(), ShellScreen::ExitConfirm);
    back(&mut state, true);
    assert_eq!(state.active_page_id(), launcher);
    assert_eq!(state.active_popup(), Some(popup));
    back(&mut state, false);
    assert!(state.active_popup().is_none());
    assert_eq!(state.active_page_id(), launcher);

    let critical = state.notify_critical_modal("Session notice", "Keep this open", vec![]);
    assert_eq!(state.notification_active_modal_id(), Some(critical));
    let title = state.frame_layout.unwrap().title;
    let point = (title.x + 1, title.y);
    state.apply_input(InputEvent::mouse_down(PointerButton::Left, point));
    state.apply_input(InputEvent::mouse_up(PointerButton::Left, point));
    assert_eq!(
        state.notification_active_modal_component(),
        Some(ShellComponent::ExitDialog)
    );
    back(&mut state, false);
    assert_eq!(state.active_page_id(), launcher);
    assert_eq!(state.notification_active_modal_id(), Some(critical));
}

#[test]
fn setup_goes_to_each_previous_step_and_stays_on_language() {
    for platform in [PlatformKind::Linux, PlatformKind::Windows] {
        let mut state = state();
        while state.notification_dismiss_active_modal_without_response() {}
        state.platform_kind = platform;
        state.reset_navigation(ShellScreen::FirstRunSetup);
        state.pages.setup.setup_step = ui::SetupStep::Appearance;
        back(&mut state, true);
        if platform == PlatformKind::Windows {
            assert_eq!(state.pages.setup.setup_step, ui::SetupStep::Timezone);
            back(&mut state, false);
        }
        assert_eq!(state.pages.setup.setup_step, ui::SetupStep::Language);
        assert!(state.header_back_label().is_none());
        back(&mut state, false);
        assert_eq!(state.active_screen(), ShellScreen::FirstRunSetup);
    }
}

#[test]
fn stale_back_region_cannot_activate_the_power_menu() {
    let mut state = state();
    while state.notification_dismiss_active_modal_without_response() {}
    state.pages.home.editing = true;
    state.refresh_hit_map();
    let area = state.frame_layout.unwrap().back_button.unwrap();
    state.pages.home.editing = false;
    let point = (area.x, area.y);
    state.apply_input(InputEvent::mouse_down(PointerButton::Left, point));
    state.apply_input(InputEvent::mouse_up(PointerButton::Left, point));
    assert_eq!(state.active_screen(), ShellScreen::Home);
    back(&mut state, false);
    assert_eq!(state.active_screen(), ShellScreen::ExitConfirm);
}
