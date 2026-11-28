use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use platform::mock::{MockCall, MockPlatform};
use platform::{
    AppPaths, PlatformCapabilities, PlatformKind, SystemCommandShell, UserDirs,
    build_windows_app_paths, cleanup_temp_path,
};
use ratatui::layout::Rect;
use shell::{
    HomeModeOverride, InputEvent, InputKey, InputModifiers, InputPhase, KeyInput,
    ShellLaunchConfig, ShellScreen, ShellSession, prepare_shell_startup,
};
use storage::{BorderShape, IconDisplayMode, MotionPreference, StorageManager, TimeSyncSource};
use ui::{SettingsApplication, SettingsCategory, SettingsField, SettingsPickerKind};

fn default_config() -> ShellLaunchConfig {
    ShellLaunchConfig {
        home_mode_override: HomeModeOverride::BuildDefault,
    }
}

#[test]
fn settings_action_repeat_is_ignored_and_shift_tab_goes_to_previous_control() {
    let fixture = FixtureRoot::new("settings-key-phases");
    let platform = mock_platform(fixture.path());
    initialize_profiles(&platform);
    let mut state = start_as_system_user(&platform, "AdminUser");
    open_settings_from_home(&mut state, &platform);
    for phase in [InputPhase::Repeat, InputPhase::Release] {
        for key in [InputKey::Enter, InputKey::Char(' ')] {
            state.apply_input_with_platform(
                InputEvent::Key(KeyInput::with_phase(key, InputModifiers::none(), phase)),
                &platform,
            );
            assert!(state.to_settings_view_model().unwrap().picker.is_none());
        }
    }
    press(&mut state, &platform, "Shift+Tab");
    assert_eq!(
        state.to_settings_view_model().unwrap().focus,
        ui::SettingsFocus::OperatingSystem
    );
    assert_eq!(
        state.to_settings_view_model().unwrap().selected_category,
        SettingsCategory::Appearance
    );
    press(&mut state, &platform, "Tab");
    assert_eq!(
        state.to_settings_view_model().unwrap().focus,
        ui::SettingsFocus::Content
    );
    press(&mut state, &platform, "Enter");
    assert_eq!(
        state.to_settings_view_model().unwrap().picker.unwrap().kind,
        SettingsPickerKind::Theme
    );
    press(&mut state, &platform, "Esc");
    assert_eq!(state.active_screen(), ShellScreen::Settings);
}

#[test]
fn settings_arrows_do_not_change_saved_values() {
    let fixture = FixtureRoot::new("settings-navigation-only");
    let platform = mock_platform(fixture.path());
    let manager = initialize_profiles(&platform);
    let mut state = start_as_system_user(&platform, "AdminUser");
    open_settings_from_home(&mut state, &platform);
    select_category(&mut state, &platform, SettingsCategory::System);
    let before = manager.load_config().unwrap();
    for key in ["Right", "Left", "Down", "Right", "Left", "Up"] {
        press(&mut state, &platform, key);
        assert_eq!(manager.load_config().unwrap(), before, "{key}");
    }
}

#[test]
fn both_reset_buttons_open_separate_confirmations_with_cancel_selected() {
    let fixture = FixtureRoot::new("settings-reset");
    let platform = mock_platform(fixture.path());
    let manager = initialize_profiles(&platform);
    let mut state = start_as_system_user(&platform, "NormalUser");
    open_settings_from_home(&mut state, &platform);
    select_category(&mut state, &platform, SettingsCategory::Reset);
    let model = state.to_settings_view_model().unwrap();
    let items = &model.cards[0].items;
    assert_eq!(items.len(), 2);
    assert_eq!(items[0].field, SettingsField::ResetAllSettings);
    assert_eq!(items[1].field, SettingsField::ResetUserData);
    let config = manager.load_config().unwrap();
    for item in items {
        assert!(item.enabled);
        assert_eq!(
            state.to_settings_view_model().unwrap().selected_field,
            item.field
        );
        press(&mut state, &platform, "Enter");
        let prompt = state.to_notification_view_model().unwrap();
        assert_eq!(prompt.title, item.label);
        assert!(
            prompt
                .actions
                .iter()
                .any(|action| action.cancel && action.selected)
        );
        press(&mut state, &platform, "Enter");
        assert!(state.to_notification_view_model().is_none());
        assert_eq!(manager.load_config().unwrap(), config);
        assert!(manager.load_profile("NormalUser").unwrap().is_some());
        press(&mut state, &platform, "Down");
    }
}

#[test]
fn numeric_settings_wait_for_confirmation_and_escape_discards_changes() {
    let fixture = FixtureRoot::new("settings-numeric-confirmation");
    let platform = mock_platform(fixture.path());
    let manager = initialize_profiles(&platform);
    let mut state = start_as_system_user(&platform, "AdminUser");
    open_settings_from_home(&mut state, &platform);
    for _ in 0..5 {
        press(&mut state, &platform, "Down");
    }
    let saved_speed = || {
        manager
            .load_profile("AdminUser")
            .unwrap()
            .unwrap()
            .appearance
            .animation_speed_percent
    };
    press(&mut state, &platform, " ");
    assert!(
        state
            .to_settings_view_model()
            .unwrap()
            .number_editor
            .is_none()
    );
    assert_eq!(saved_speed(), 100);
    press(&mut state, &platform, "Enter");
    assert_eq!(saved_speed(), 100);
    press(&mut state, &platform, "Right");
    press(&mut state, &platform, "Left");
    let input = state
        .to_settings_view_model()
        .unwrap()
        .number_editor
        .unwrap()
        .input;
    assert_eq!(input.value(), "100");
    assert_eq!(input.cursor(), 2);
    adjust_number(&mut state, &platform, true);
    assert_eq!(saved_speed(), 100);
    press(&mut state, &platform, "Enter");
    assert_eq!(saved_speed(), 125);
    assert_eq!(
        state.to_settings_view_model().unwrap().selected_field,
        SettingsField::AnimationSpeed
    );
    for key in ["Enter", "Left", "Esc"] {
        press(&mut state, &platform, key);
    }
    assert_eq!(saved_speed(), 125);
    assert_eq!(state.active_screen(), ShellScreen::Settings);
}

#[test]
fn tab_traverses_controls_and_category_arrows_do_not_change_values() {
    let fixture = FixtureRoot::new("settings-focus");
    let platform = mock_platform(fixture.path());
    let manager = initialize_profiles(&platform);
    let mut state = start_as_system_user(&platform, "AdminUser");
    open_settings_from_home(&mut state, &platform);
    let before = manager.load_config().unwrap();
    for key in ["Tab", "Shift+Tab", "Down", "Up"] {
        press(&mut state, &platform, key);
        let model = state.to_settings_view_model().unwrap();
        assert_eq!(model.selected_category, SettingsCategory::Appearance);
        assert_eq!(model.focus, ui::SettingsFocus::Content);
        assert_eq!(manager.load_config().unwrap(), before);
    }
    press(&mut state, &platform, "Shift+Tab");
    press(&mut state, &platform, "Shift+Tab");
    assert_eq!(
        state.to_settings_view_model().unwrap().focus,
        ui::SettingsFocus::Categories
    );
    for (key, category) in [
        ("Down", SettingsCategory::RegionTime),
        ("Down", SettingsCategory::System),
        ("Up", SettingsCategory::RegionTime),
    ] {
        press(&mut state, &platform, key);
        let model = state.to_settings_view_model().unwrap();
        assert_eq!(model.selected_category, category);
        assert_eq!(model.focus, ui::SettingsFocus::Categories);
        assert_eq!(manager.load_config().unwrap(), before);
    }
    press(&mut state, &platform, "Enter");
    assert_eq!(
        state.to_settings_view_model().unwrap().focus,
        ui::SettingsFocus::Content
    );
    assert_eq!(
        state.to_settings_view_model().unwrap().selected_field,
        SettingsField::Language
    );
    press(&mut state, &platform, "Tab");
    assert_eq!(
        state.to_settings_view_model().unwrap().selected_field,
        SettingsField::Timezone
    );
    assert!(state.to_settings_view_model().unwrap().picker.is_none());
    assert_eq!(manager.load_config().unwrap(), before);
}

#[test]
fn numeric_settings_validate_direct_input_before_saving() {
    let fixture = FixtureRoot::new("settings-number-input");
    let platform = mock_platform(fixture.path());
    let manager = initialize_profiles(&platform);
    let mut state = start_as_system_user(&platform, "AdminUser");
    open_settings_from_home(&mut state, &platform);
    for (category, field, value, display) in [
        (
            SettingsCategory::System,
            SettingsField::SystemLowAvailable,
            "20",
            "20 GiB",
        ),
        (
            SettingsCategory::System,
            SettingsField::SystemLowPercentage,
            "25",
            "25%",
        ),
        (
            SettingsCategory::System,
            SettingsField::SystemCriticalAvailable,
            "3",
            "3 GiB",
        ),
        (
            SettingsCategory::System,
            SettingsField::SystemCriticalPercentage,
            "8",
            "8%",
        ),
        (
            SettingsCategory::Applications,
            SettingsField::CursorDelay,
            "1000",
            "1000 ms",
        ),
        (
            SettingsCategory::Applications,
            SettingsField::CursorRamp,
            "2000",
            "2000 ms",
        ),
        (
            SettingsCategory::Applications,
            SettingsField::CursorHorizontalStep,
            "12",
            "12 cells",
        ),
        (
            SettingsCategory::Applications,
            SettingsField::CursorVerticalStep,
            "5",
            "5 lines",
        ),
    ] {
        select_category(&mut state, &platform, category);
        if category == SettingsCategory::Applications
            && state.to_settings_view_model().unwrap().selected_application
                != SettingsApplication::Editor
        {
            press(&mut state, &platform, "Home");
            press(&mut state, &platform, "Down");
        }
        select_field(&mut state, &platform, field);
        press(&mut state, &platform, "Enter");
        let before = manager.load_config().unwrap();
        let model = state.to_settings_view_model().unwrap();
        let editor = model.number_editor.unwrap();
        for invalid in [String::new(), "0".into(), (editor.maximum + 1).to_string()] {
            press(&mut state, &platform, "Ctrl+A");
            press(&mut state, &platform, "Backspace");
            for digit in invalid.chars() {
                press(&mut state, &platform, &digit.to_string());
            }
            press(&mut state, &platform, "Enter");
            assert!(
                state
                    .to_settings_view_model()
                    .unwrap()
                    .number_editor
                    .unwrap()
                    .error
                    .is_some(),
                "{field:?}"
            );
            assert_eq!(manager.load_config().unwrap(), before);
        }
        press(&mut state, &platform, "Ctrl+A");
        for digit in value.chars() {
            press(&mut state, &platform, &digit.to_string());
        }
        assert_eq!(manager.load_config().unwrap(), before);
        press(&mut state, &platform, "Enter");
        let model = state.to_settings_view_model().unwrap();
        assert!(model.number_editor.is_none());
        assert_eq!(model.selected_field, field);
        assert_eq!(
            model
                .cards
                .iter()
                .flat_map(|card| &card.items)
                .find(|item| item.field == field)
                .unwrap()
                .value,
            display
        );
    }
}

#[test]
fn numeric_settings_use_the_same_dialog_for_mouse_and_touch() {
    for hover in [true, false] {
        let fixture = FixtureRoot::new("settings-number-pointer");
        let platform = mock_platform(fixture.path());
        let manager = initialize_profiles(&platform);
        let mut state = start_as_system_user(&platform, "AdminUser");
        open_settings_from_home(&mut state, &platform);
        select_category(&mut state, &platform, SettingsCategory::System);
        let main = match ui::compute_shell_layout(Rect::new(0, 0, 120, 40)) {
            ui::ShellLayout::Compact(main) | ui::ShellLayout::Full { main, .. } => main,
        };
        for confirm in [true, false] {
            let before = manager.load_config().unwrap();
            let layout = ui::settings_layout(main, &state.to_settings_view_model().unwrap());
            let field = layout
                .fields
                .iter()
                .find(|field| field.field == SettingsField::SystemLowAvailable)
                .unwrap()
                .area;
            let point = (field.x, field.y);
            if hover {
                state.apply_input_with_platform(InputEvent::mouse_moved(point), &platform);
            }
            state.apply_input_with_platform(
                InputEvent::mouse_down(ui::MouseButton::Left, point),
                &platform,
            );
            state.apply_input_with_platform(
                InputEvent::mouse_up(ui::MouseButton::Left, point),
                &platform,
            );
            let model = state.to_settings_view_model().unwrap();
            assert_eq!(
                model.number_editor.as_ref().unwrap().field,
                SettingsField::SystemLowAvailable
            );
            let layout = ui::settings_layout(main, &model);
            let number = layout.number_editor.unwrap();
            let point = (number.increase.x, number.increase.y);
            state.apply_input_with_platform(
                InputEvent::mouse_down(ui::MouseButton::Left, point),
                &platform,
            );
            state.apply_input_with_platform(
                InputEvent::mouse_up(ui::MouseButton::Left, point),
                &platform,
            );
            assert_eq!(manager.load_config().unwrap(), before);
            assert_eq!(
                state
                    .to_settings_view_model()
                    .unwrap()
                    .number_editor
                    .unwrap()
                    .input
                    .value(),
                (before.system_status.low_available_gib + 1).to_string()
            );
            let background = layout.category_cards[0].area;
            state.apply_input_with_platform(
                InputEvent::mouse_down(ui::MouseButton::Left, (background.x, background.y)),
                &platform,
            );
            state.apply_input_with_platform(
                InputEvent::mouse_up(ui::MouseButton::Left, (background.x, background.y)),
                &platform,
            );
            assert_eq!(
                state.to_settings_view_model().unwrap().selected_category,
                SettingsCategory::System
            );
            let button = if confirm {
                layout.overlay_apply.unwrap()
            } else {
                layout.overlay_cancel.unwrap()
            };
            let point = (button.x, button.y);
            state.apply_input_with_platform(
                InputEvent::mouse_down(ui::MouseButton::Left, point),
                &platform,
            );
            state.apply_input_with_platform(
                InputEvent::mouse_up(ui::MouseButton::Left, point),
                &platform,
            );
            let model = state.to_settings_view_model().unwrap();
            assert!(model.number_editor.is_none());
            assert_eq!(model.selected_field, SettingsField::SystemLowAvailable);
            assert_eq!(
                manager
                    .load_config()
                    .unwrap()
                    .system_status
                    .low_available_gib,
                before.system_status.low_available_gib + u16::from(confirm)
            );
        }
    }
}

#[cfg(target_os = "linux")]
#[test]
fn linux_personal_settings_work_without_system_account_service() {
    const CHILD: &str = "TUNDRA_TEST_SETTINGS_WITHOUT_ACCOUNT_SERVICE";
    let fixture = FixtureRoot::new("linux-no-account-service");
    if std::env::var_os(CHILD).is_none() {
        // Change the bus address only in an isolated test process, never while
        // other tests may be using the environment or the real system bus.
        let result = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "linux_personal_settings_work_without_system_account_service",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .env(
                "DBUS_SYSTEM_BUS_ADDRESS",
                format!("unix:path={}/missing-bus", fixture.path().display()),
            )
            .status()
            .unwrap();
        assert!(result.success());
        return;
    }
    let platform = mock_platform(fixture.path()).with_kind(PlatformKind::Linux);
    let manager = initialize_profiles(&platform);
    let mut state = start_as_system_user(&platform, "AdminUser");
    open_settings_from_home(&mut state, &platform);
    assert_eq!(state.active_screen(), ShellScreen::Settings);

    // Exercise the normal settings save path, then reopen from disk.
    let before = manager
        .load_profile("AdminUser")
        .unwrap()
        .unwrap()
        .appearance
        .border_shape;
    press(&mut state, &platform, "Down");
    press(&mut state, &platform, "Enter");
    let saved = manager
        .load_profile("AdminUser")
        .unwrap()
        .unwrap()
        .appearance
        .clone();
    assert_ne!(saved.border_shape, before);
    let mut state = start_as_system_user(&platform, "AdminUser");
    open_settings_from_home(&mut state, &platform);
    assert_eq!(state.active_screen(), ShellScreen::Settings);
    assert_eq!(
        manager
            .load_profile("AdminUser")
            .unwrap()
            .unwrap()
            .appearance,
        saved
    );
}

#[test]
fn category_navigation_keeps_application_list_and_content_arrows() {
    let fixture = FixtureRoot::new("tab-section-cycle");
    let platform = mock_platform(fixture.path());
    initialize_profiles(&platform);
    let mut state = start_as_system_user(&platform, "AdminUser");

    open_settings_from_home(&mut state, &platform);
    let appearance = state.to_settings_view_model().unwrap();
    assert_eq!(appearance.selected_category, SettingsCategory::Appearance);
    assert_eq!(appearance.selected_field, SettingsField::Theme);

    press(&mut state, &platform, "Down");
    let appearance = state.to_settings_view_model().unwrap();
    assert_eq!(appearance.selected_category, SettingsCategory::Appearance);
    assert_eq!(appearance.selected_field, SettingsField::BorderShape);

    for expected in [
        SettingsCategory::RegionTime,
        SettingsCategory::System,
        SettingsCategory::Applications,
        SettingsCategory::Update,
        SettingsCategory::Appearance,
    ] {
        select_category(&mut state, &platform, expected);
        assert_eq!(
            state.to_settings_view_model().unwrap().selected_category,
            expected
        );
    }
    assert_eq!(
        state.to_settings_view_model().unwrap().selected_field,
        SettingsField::Theme
    );

    select_category(&mut state, &platform, SettingsCategory::Update);
    let update = state.to_settings_view_model().unwrap();
    assert_eq!(update.selected_category, SettingsCategory::Update);
    assert_eq!(
        update.selected_field,
        if cfg!(target_os = "linux") {
            SettingsField::UpdateMode
        } else {
            SettingsField::InstalledVersion
        }
    );

    select_category(&mut state, &platform, SettingsCategory::Applications);
    let applications = state.to_settings_view_model().unwrap();
    assert_eq!(
        applications.selected_category,
        SettingsCategory::Applications
    );
    assert_eq!(applications.selected_field, SettingsField::ApplicationList);
    assert_eq!(
        applications.selected_application,
        SettingsApplication::FileExplorer
    );
    press(&mut state, &platform, "Down");
    press(&mut state, &platform, "Enter");
    let editor = state.to_settings_view_model().unwrap();
    assert_eq!(editor.selected_category, SettingsCategory::Applications);
    assert_eq!(editor.selected_application, SettingsApplication::Editor);
    assert_eq!(editor.selected_field, SettingsField::ExplorerOpenExtensions);

    press(&mut state, &platform, "Down");
    let editor = state.to_settings_view_model().unwrap();
    assert_eq!(editor.selected_category, SettingsCategory::Applications);
    assert_eq!(editor.selected_field, SettingsField::CursorAcceleration);
}

#[cfg(target_os = "linux")]
#[test]
fn linux_update_mode_saves_keyboard_and_mouse_choices_across_reopen() {
    let fixture = FixtureRoot::new("update-mode");
    let platform = mock_platform(fixture.path());
    let manager = initialize_profiles(&platform);
    let mut state = start_as_system_user(&platform, "AdminUser");
    assert_eq!(
        manager.load_config().unwrap().linux_update_mode,
        storage::LinuxUpdateMode::Release
    );
    open_settings_from_home(&mut state, &platform);
    select_category(&mut state, &platform, SettingsCategory::Update);
    assert_eq!(
        state.to_settings_view_model().unwrap().selected_field,
        SettingsField::UpdateMode
    );
    press(&mut state, &platform, "Enter");
    assert_eq!(
        manager.load_config().unwrap().linux_update_mode,
        storage::LinuxUpdateMode::Beta
    );
    let mut state = start_as_system_user(&platform, "AdminUser");
    open_settings_from_home(&mut state, &platform);
    select_category(&mut state, &platform, SettingsCategory::Update);
    let model = state.to_settings_view_model().unwrap();
    assert!(
        model
            .cards
            .iter()
            .flat_map(|c| &c.items)
            .any(|i| i.field == SettingsField::UpdateMode && i.value.contains("Beta"))
    );
    let main = match ui::compute_shell_layout(Rect::new(0, 0, 120, 40)) {
        ui::ShellLayout::Compact(main) | ui::ShellLayout::Full { main, .. } => main,
    };
    let layout = ui::settings_layout(main, &model);
    let field = layout
        .fields
        .iter()
        .find(|f| f.field == SettingsField::UpdateMode)
        .unwrap();
    state.apply_input_with_platform(
        InputEvent::mouse_down(ui::MouseButton::Left, (field.area.x, field.area.y)),
        &platform,
    );
    assert_eq!(
        manager.load_config().unwrap().linux_update_mode,
        storage::LinuxUpdateMode::Release
    );
    press(&mut state, &platform, "Enter");
    assert_eq!(
        manager.load_config().unwrap().linux_update_mode,
        storage::LinuxUpdateMode::Beta
    );
    press(&mut state, &platform, "Enter");
    assert_eq!(
        manager.load_config().unwrap().linux_update_mode,
        storage::LinuxUpdateMode::Release
    );
}

#[test]
fn application_list_switches_with_pointer_without_changing_saved_settings() {
    let fixture = FixtureRoot::new("application-settings-pointer");
    let platform = mock_platform(fixture.path());
    let manager = initialize_profiles(&platform);
    let mut state = start_as_system_user(&platform, "AdminUser");
    open_settings_from_home(&mut state, &platform);
    select_category(&mut state, &platform, SettingsCategory::Applications);
    let original = manager.load_config().unwrap();
    let ui::ShellLayout::Full { main, .. } = ui::compute_shell_layout(Rect::new(0, 0, 120, 40))
    else {
        panic!("full layout");
    };
    for application in [
        SettingsApplication::Editor,
        SettingsApplication::CommandLine,
        SettingsApplication::FileExplorer,
    ] {
        let model = state.to_settings_view_model().unwrap();
        let layout = ui::settings_layout(main, &model);
        let button = layout
            .applications
            .as_ref()
            .unwrap()
            .options
            .iter()
            .find(|(candidate, _)| *candidate == application)
            .unwrap()
            .1;
        let point = (button.x, button.y);
        state.apply_input_with_platform(
            InputEvent::mouse_down(ui::MouseButton::Left, point),
            &platform,
        );
        state.apply_input_with_platform(
            InputEvent::mouse_up(ui::MouseButton::Left, point),
            &platform,
        );
        let model = state.to_settings_view_model().unwrap();
        assert_eq!(model.selected_application, application);
        assert_eq!(model.selected_field, SettingsField::ApplicationList);
        let first = model.cards[0].items[0].field;
        assert_eq!(
            first,
            match application {
                SettingsApplication::FileExplorer => SettingsField::ShowHidden,
                SettingsApplication::Editor => SettingsField::ExplorerOpenExtensions,
                SettingsApplication::CommandLine => SettingsField::CommandLineShell,
            }
        );
        assert_eq!(manager.load_config().unwrap(), original);
    }
}

#[test]
fn restore_application_defaults_changes_only_the_selected_application() {
    let fixture = FixtureRoot::new("application-settings-reset");
    let platform = mock_platform(fixture.path());
    let manager = initialize_profiles(&platform);
    let mut config = manager.load_config().unwrap();
    config.explorer.show_hidden = true;
    config.editor.cursor_acceleration_delay_ms += 100;
    manager.save_config(&config).unwrap();
    let mut state = start_as_system_user(&platform, "AdminUser");
    open_settings_from_home(&mut state, &platform);
    select_category(&mut state, &platform, SettingsCategory::Applications);
    for key in ["End", "Enter", "R"] {
        press(&mut state, &platform, key);
    }
    let saved = manager.load_config().unwrap();
    assert_eq!(saved.explorer, storage::ExplorerConfig::default());
    assert_eq!(saved.editor, config.editor);
    for key in ["Home", "Down", "Enter", "End", "Enter", "R"] {
        press(&mut state, &platform, key);
    }
    let saved = manager.load_config().unwrap();
    assert_eq!(saved.editor, storage::EditorConfig::default());
    assert_eq!(saved.explorer, storage::ExplorerConfig::default());
    assert_eq!(
        state.to_settings_view_model().unwrap().selected_application,
        SettingsApplication::Editor
    );
}

#[test]
fn mouse_wheel_scrolls_only_to_the_settings_content_boundary() {
    let fixture = FixtureRoot::new("bounded-settings-scroll");
    let platform = mock_platform(fixture.path());
    initialize_profiles(&platform);
    let mut state = start_as_system_user(&platform, "AdminUser");
    open_settings_from_home(&mut state, &platform);

    state.apply_input_with_platform(
        InputEvent::Resize {
            width: 120,
            height: 60,
        },
        &platform,
    );
    for _ in 0..20 {
        state.apply_input_with_platform(
            InputEvent::mouse_scroll(shell::ScrollDirection::Down, (60, 10)),
            &platform,
        );
    }
    let fitting_model = state.to_settings_view_model().expect("settings model");
    let fitting_main = match ui::compute_shell_layout(Rect::new(0, 0, 120, 60)) {
        ui::ShellLayout::Compact(compact) => compact,
        ui::ShellLayout::Full { main, .. } => main,
    };
    let fitting_layout = ui::settings_layout(fitting_main, &fitting_model);
    assert_eq!(fitting_model.scroll_offset, 0);
    assert_eq!(fitting_layout.max_scroll_offset, 0);
    assert!(fitting_layout.scrollbar.is_none());

    state.apply_input_with_platform(
        InputEvent::Resize {
            width: 120,
            height: 12,
        },
        &platform,
    );
    let overflowing_model = state.to_settings_view_model().expect("settings model");
    let overflowing_main = match ui::compute_shell_layout(Rect::new(0, 0, 120, 12)) {
        ui::ShellLayout::Compact(compact) => compact,
        ui::ShellLayout::Full { main, .. } => main,
    };
    let overflowing_layout = ui::settings_layout(overflowing_main, &overflowing_model);
    assert!(overflowing_layout.scrollbar.is_some());
    assert!(overflowing_layout.max_scroll_offset > 0);

    for direction in [shell::ScrollDirection::Left, shell::ScrollDirection::Right] {
        state.apply_input_with_platform(InputEvent::mouse_scroll(direction, (60, 6)), &platform);
        assert_eq!(state.to_settings_view_model().unwrap().scroll_offset, 0);
    }

    for _ in 0..100 {
        state.apply_input_with_platform(
            InputEvent::mouse_scroll(shell::ScrollDirection::Down, (60, 6)),
            &platform,
        );
    }
    assert_eq!(
        state
            .to_settings_view_model()
            .expect("scrolled settings model")
            .scroll_offset,
        overflowing_layout.max_scroll_offset
    );

    state.apply_input_with_platform(
        InputEvent::Resize {
            width: 120,
            height: 60,
        },
        &platform,
    );
    let resized_model = state
        .to_settings_view_model()
        .expect("resized settings model");
    let resized_layout = ui::settings_layout(fitting_main, &resized_model);
    assert_eq!(resized_model.scroll_offset, 0);
    assert!(resized_layout.scrollbar.is_none());
}

#[test]
fn system_settings_show_defaults_and_persist_confirmed_values() {
    let fixture = FixtureRoot::new("system-settings");
    let platform = mock_platform(fixture.path());
    let manager = initialize_profiles(&platform);
    let mut state = start_as_system_user(&platform, "AdminUser");
    open_settings_from_home(&mut state, &platform);
    select_category(&mut state, &platform, SettingsCategory::System);
    for (index, policy) in [
        (2, storage::AutoAdminPolicy::Automatic),
        (0, storage::AutoAdminPolicy::Deny),
        (1, storage::AutoAdminPolicy::Manual),
    ] {
        let before = manager.load_config().unwrap().auto_admin;
        press(&mut state, &platform, "Enter");
        press(&mut state, &platform, "Home");
        for _ in 0..index {
            press(&mut state, &platform, "Down");
        }
        assert_eq!(manager.load_config().unwrap().auto_admin, before);
        press(&mut state, &platform, "Enter");
        assert_eq!(manager.load_config().unwrap().auto_admin, policy);
    }
    press(&mut state, &platform, "Down");
    press(&mut state, &platform, "Enter");
    assert!(manager.load_config().unwrap().automation.enabled);
    press(&mut state, &platform, " ");
    assert!(!manager.load_config().unwrap().automation.enabled);
    for _ in 0..2 {
        press(&mut state, &platform, "Down");
    }
    let model = state.to_settings_view_model().unwrap();
    let items = &model.cards[3].items;
    assert_eq!(
        items
            .iter()
            .map(|item| item.value.as_str())
            .collect::<Vec<_>>(),
        ["5 GiB", "10%", "1 GiB", "5%"]
    );
    assert_eq!(model.selected_field, SettingsField::SystemLowAvailable);
    press(&mut state, &platform, "Enter");
    adjust_number(&mut state, &platform, false);
    press(&mut state, &platform, "Enter");
    assert_eq!(
        manager
            .load_config()
            .unwrap()
            .system_status
            .low_available_gib,
        4
    );

    let mut config = manager.load_config().unwrap();
    config.system_status.low_available_gib = 5;
    config.system_status.critical_available_gib = 5;
    manager.save_config(&config).unwrap();
    for key in ["Enter", "Ctrl+A", "4", "Enter"] {
        press(&mut state, &platform, key);
    }
    let saved = manager.load_config().unwrap();
    assert_eq!(saved.system_status.low_available_gib, 4);
    assert_eq!(saved.system_status.critical_available_gib, 4);
    for key in ["Down", "Down", "Enter"] {
        press(&mut state, &platform, key);
    }
    adjust_number(&mut state, &platform, true);
    press(&mut state, &platform, "Enter");
    assert_eq!(
        manager
            .load_config()
            .unwrap()
            .system_status
            .critical_available_gib,
        4
    );
}

#[test]
fn auto_admin_picker_requires_confirmation_with_mouse_or_touch() {
    for hover in [true, false] {
        let fixture = FixtureRoot::new("aa-picker");
        let platform = mock_platform(fixture.path());
        let manager = initialize_profiles(&platform);
        for username in ["AdminUser", "NormalUser"] {
            let mut state = start_as_system_user(&platform, username);
            open_settings_from_home(&mut state, &platform);
            select_category(&mut state, &platform, SettingsCategory::System);
            let main = match ui::compute_shell_layout(Rect::new(0, 0, 120, 40)) {
                ui::ShellLayout::Compact(main) | ui::ShellLayout::Full { main, .. } => main,
            };
            for (index, policy) in [
                (2, storage::AutoAdminPolicy::Automatic),
                (0, storage::AutoAdminPolicy::Deny),
                (1, storage::AutoAdminPolicy::Manual),
            ] {
                let before = manager.load_config().unwrap().auto_admin;
                let layout = ui::settings_layout(main, &state.to_settings_view_model().unwrap());
                let area = layout
                    .fields
                    .iter()
                    .find(|field| field.field == SettingsField::AutoAdmin)
                    .unwrap()
                    .area;
                let point = (area.x, area.y);
                if hover {
                    state.apply_input_with_platform(InputEvent::mouse_moved(point), &platform);
                }
                state.apply_input_with_platform(
                    InputEvent::mouse_down(ui::MouseButton::Left, point),
                    &platform,
                );
                state.apply_input_with_platform(
                    InputEvent::mouse_up(ui::MouseButton::Left, point),
                    &platform,
                );
                let model = state.to_settings_view_model().unwrap();
                assert_eq!(
                    model.picker.as_ref().unwrap().kind,
                    SettingsPickerKind::AutoAdmin
                );
                let layout = ui::settings_layout(main, &model);
                let option = layout
                    .picker_options
                    .iter()
                    .find(|option| option.index == index)
                    .unwrap()
                    .area;
                let point = (option.x, option.y);
                if hover {
                    state.apply_input_with_platform(InputEvent::mouse_moved(point), &platform);
                }
                state.apply_input_with_platform(
                    InputEvent::mouse_down(ui::MouseButton::Left, point),
                    &platform,
                );
                state.apply_input_with_platform(
                    InputEvent::mouse_up(ui::MouseButton::Left, point),
                    &platform,
                );
                assert_eq!(manager.load_config().unwrap().auto_admin, before);
                assert_eq!(
                    state
                        .to_settings_view_model()
                        .unwrap()
                        .picker
                        .unwrap()
                        .selected_index,
                    index
                );
                let apply = layout.overlay_apply.unwrap();
                let point = (apply.x, apply.y);
                state.apply_input_with_platform(
                    InputEvent::mouse_down(ui::MouseButton::Left, point),
                    &platform,
                );
                state.apply_input_with_platform(
                    InputEvent::mouse_up(ui::MouseButton::Left, point),
                    &platform,
                );
                assert_eq!(manager.load_config().unwrap().auto_admin, policy);
                assert!(state.to_settings_view_model().unwrap().picker.is_none());
            }
        }
    }
}

#[test]
fn command_line_shell_picker_filters_saves_and_restores_defaults() {
    let fixture = FixtureRoot::new("command-line-shell");
    let platform = mock_platform(fixture.path()).with_kind(platform::native_platform().kind());
    let manager = initialize_profiles(&platform);
    let mut state = start_as_system_user(&platform, "AdminUser");
    open_settings_from_home(&mut state, &platform);
    select_category(&mut state, &platform, SettingsCategory::Applications);
    for key in ["Down", "Down", "Right"] {
        press(&mut state, &platform, key);
    }
    let model = state.to_settings_view_model().unwrap();
    assert_eq!(model.selected_application, SettingsApplication::CommandLine);
    assert_eq!(model.selected_field, SettingsField::CommandLineShell);
    press(&mut state, &platform, "Enter");
    let picker = state.to_settings_view_model().unwrap().picker.unwrap();
    assert_eq!(picker.kind, SettingsPickerKind::CommandLineShell);
    assert!(picker.searchable);
    assert_eq!(picker.selected_index, 0);
    assert_eq!(picker.options.len(), SystemCommandShell::choices().len());

    let selected = *SystemCommandShell::choices().last().unwrap();
    for character in selected.program().unwrap().chars() {
        press(&mut state, &platform, &character.to_string());
    }
    let picker = state.to_settings_view_model().unwrap().picker.unwrap();
    assert_eq!(picker.options.len(), 1);
    assert_eq!(
        manager.load_config().unwrap().command_line.shell,
        SystemCommandShell::Default
    );
    press(&mut state, &platform, "Enter");
    assert_eq!(manager.load_config().unwrap().command_line.shell, selected);
    press(&mut state, &platform, "Enter");
    let picker = state.to_settings_view_model().unwrap().picker.unwrap();
    assert_eq!(
        picker.selected_index,
        SystemCommandShell::choices().len() - 1
    );
    for key in ["Esc", "Down", "Enter", "r"] {
        press(&mut state, &platform, key);
    }
    assert_eq!(
        manager.load_config().unwrap().command_line.shell,
        SystemCommandShell::Default
    );
}

#[test]
fn top_bar_settings_save_alignment_and_apply_checkbox_visibility() {
    let fixture = FixtureRoot::new("top-bar-settings");
    let platform = mock_platform(fixture.path()).with_kind(PlatformKind::Macos);
    let manager = initialize_profiles(&platform);
    let mut state = start_as_system_user(&platform, "AdminUser");
    open_settings_from_home(&mut state, &platform);
    select_category(&mut state, &platform, SettingsCategory::System);
    select_field(&mut state, &platform, SettingsField::TopBarStatusAlignment);

    let mut expected = storage::TopBarConfig::default();
    for (key, alignment) in [
        ("Home", storage::StatusTextAlignment::Left),
        ("End", storage::StatusTextAlignment::Right),
        ("Up", storage::StatusTextAlignment::Center),
    ] {
        press(&mut state, &platform, "Enter");
        press(&mut state, &platform, key);
        assert_eq!(manager.load_config().unwrap().top_bar, expected);
        press(&mut state, &platform, "Enter");
        expected.status_alignment = alignment;
        assert_eq!(manager.load_config().unwrap().top_bar, expected);
        assert!(state.to_settings_view_model().unwrap().picker.is_none());
    }

    for (field, component, preference) in [
        (
            SettingsField::TopBarHelp,
            shell::ShellComponent::HelpButton,
            &mut expected.show_help,
        ),
        (
            SettingsField::TopBarTime,
            shell::ShellComponent::ClockButton,
            &mut expected.show_time,
        ),
        (
            SettingsField::TopBarEsc,
            shell::ShellComponent::BackButton,
            &mut expected.show_esc,
        ),
    ] {
        select_field(&mut state, &platform, field);
        let model = state.to_settings_view_model().unwrap();
        let item = model
            .cards
            .iter()
            .flat_map(|card| &card.items)
            .find(|item| item.field == field)
            .unwrap();
        assert_eq!(item.checked, Some(*preference));
        press(&mut state, &platform, " ");
        *preference = !*preference;
        assert_eq!(
            state
                .hit_map()
                .regions()
                .iter()
                .any(|region| region.component == component),
            *preference,
        );
    }
    assert_eq!(manager.load_config().unwrap().top_bar, expected);
    let reopened = start_as_system_user(&platform, "AdminUser");
    let chrome = reopened.to_shell_chrome_view_model();
    assert!(chrome.status.time_button_label.is_none());
    assert!(!chrome.help_available);
    press(&mut state, &platform, "Esc");
    assert_eq!(state.active_screen(), ShellScreen::Settings);
    press(&mut state, &platform, "F1");
    assert!(
        state
            .hit_map()
            .regions()
            .iter()
            .any(|region| region.component == shell::ShellComponent::HelpDialog)
    );
}

#[test]
fn screen_timeout_saves_all_choices_and_wakes_without_logging_out() {
    let fixture = FixtureRoot::new("screen-timeout");
    let platform = mock_platform(fixture.path());
    let manager = initialize_profiles(&platform);
    let mut state = start_as_system_user(&platform, "AdminUser");
    open_settings_from_home(&mut state, &platform);
    select_category(&mut state, &platform, SettingsCategory::System);
    for key in ["Down", "Down"] {
        press(&mut state, &platform, key);
    }
    assert_eq!(
        state.to_settings_view_model().unwrap().selected_field,
        SettingsField::ScreenTimeout
    );
    for (index, seconds) in [0, 30, 60, 120, 300, 600, 1800].into_iter().enumerate() {
        let before = manager.load_config().unwrap().screen_timeout_seconds;
        press(&mut state, &platform, "Enter");
        press(&mut state, &platform, "Home");
        for _ in 0..index {
            press(&mut state, &platform, "Down");
        }
        assert_eq!(
            manager.load_config().unwrap().screen_timeout_seconds,
            before
        );
        press(&mut state, &platform, "Enter");
        assert_eq!(
            manager.load_config().unwrap().screen_timeout_seconds,
            seconds
        );
    }
    for key in ["Enter", "Home", "Down", "Down", "Down", "Enter"] {
        press(&mut state, &platform, key);
    }
    assert_eq!(manager.load_config().unwrap().screen_timeout_seconds, 120);
    let actor = state.auth_session().cloned();
    let now = Instant::now();
    state.apply_input_at(InputEvent::FocusGained, now);
    state.apply_input_at(InputEvent::Tick, now + Duration::from_secs(120));
    assert!(state.screen_saver_active());
    state.apply_input_at(
        InputEvent::from_key_label("Esc"),
        now + Duration::from_secs(121),
    );
    assert!(!state.screen_saver_active());
    assert_eq!(state.active_screen(), ShellScreen::Settings);
    assert_eq!(state.auth_session(), actor.as_ref());
    assert!(!state.shutdown_requested());
    assert_eq!(
        state.to_settings_view_model().unwrap().selected_field,
        SettingsField::ScreenTimeout
    );
}

#[test]
fn admin_settings_immediately_persist_global_changes_and_confirm_picker_selection() {
    let fixture = FixtureRoot::new("admin-persistence");
    let platform = mock_platform(fixture.path());
    let manager = initialize_profiles(&platform);
    let mut state = start_as_system_user(&platform, "AdminUser");

    open_settings_from_home(&mut state, &platform);
    assert_eq!(state.active_screen(), ShellScreen::Settings);

    // Appearance -> Region & Time. Moving the timezone highlight must not save.
    select_category(&mut state, &platform, SettingsCategory::RegionTime);
    press(&mut state, &platform, "Down");
    assert_eq!(
        state.to_settings_view_model().unwrap().selected_field,
        SettingsField::Timezone
    );
    let original_timezone = manager.load_config().unwrap().timezone;
    press(&mut state, &platform, "Enter");
    press(&mut state, &platform, "Down");
    let highlighted_timezone = state
        .to_settings_view_model()
        .unwrap()
        .picker
        .unwrap()
        .options
        .into_iter()
        .nth(1)
        .and_then(|option| option.timezone_id)
        .expect("second timezone option");
    assert_eq!(manager.load_config().unwrap().timezone, original_timezone);
    press(&mut state, &platform, "Enter");
    assert_eq!(
        manager.load_config().unwrap().timezone,
        highlighted_timezone
    );

    select_category(&mut state, &platform, SettingsCategory::Applications);
    assert_eq!(
        state.to_settings_view_model().unwrap().selected_category,
        SettingsCategory::Applications
    );
    assert!(!manager.load_config().unwrap().explorer.show_hidden);
    press(&mut state, &platform, "Right");
    press(&mut state, &platform, "Enter");
    assert!(manager.load_config().unwrap().explorer.show_hidden);
    assert!(
        state
            .to_settings_view_model()
            .unwrap()
            .status
            .contains("Saved")
    );
    select_field(&mut state, &platform, SettingsField::SortField);
    for expected in [
        storage::ExplorerSortField::Type,
        storage::ExplorerSortField::Size,
        storage::ExplorerSortField::Modified,
        storage::ExplorerSortField::Name,
    ] {
        press(&mut state, &platform, "Enter");
        assert_eq!(manager.load_config().unwrap().explorer.sort_field, expected);
    }
}

#[test]
fn weather_location_rejects_non_english_input_and_saves_only_after_warning() {
    let fixture = FixtureRoot::new("weather-location");
    let platform = mock_platform(fixture.path());
    let manager = initialize_profiles(&platform);
    let mut state = start_as_system_user(&platform, "AdminUser");

    open_settings_from_home(&mut state, &platform);
    select_category(&mut state, &platform, SettingsCategory::RegionTime);
    for _ in 0..2 {
        press(&mut state, &platform, "Down");
    }
    assert_eq!(
        state.to_settings_view_model().unwrap().selected_field,
        SettingsField::WeatherLocation
    );
    press(&mut state, &platform, "Enter");

    press(&mut state, &platform, "杭");
    let editor = state
        .to_settings_view_model()
        .unwrap()
        .weather_location_editor
        .expect("weather location editor");
    assert!(editor.value.is_empty());
    assert!(editor.error.unwrap().contains("Only English"));

    let location = "Cambridge, Massachusetts, USA";
    for character in location.chars() {
        press(&mut state, &platform, &character.to_string());
    }
    press(&mut state, &platform, "Enter");
    assert_eq!(manager.load_config().unwrap().weather_location, None);
    let warning = state
        .to_notification_view_model()
        .expect("weather search warning");
    assert!(warning.message.contains(location));

    press(&mut state, &platform, "Enter");
    assert_eq!(
        manager.load_config().unwrap().weather_location.as_deref(),
        Some(location)
    );
    assert!(
        state
            .to_settings_view_model()
            .unwrap()
            .weather_location_editor
            .is_none()
    );
}

#[test]
fn operating_system_time_source_uses_platform_boundary_and_persists() {
    let fixture = FixtureRoot::new("system-time-source");
    let platform = mock_platform(fixture.path());
    let manager = initialize_profiles(&platform);
    let system_time = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    platform.set_system_time_result(Ok(system_time));
    let mut state = start_as_system_user(&platform, "AdminUser");

    open_settings_from_home(&mut state, &platform);
    select_category(&mut state, &platform, SettingsCategory::RegionTime);
    press(&mut state, &platform, "Down");
    press(&mut state, &platform, "Down");
    press(&mut state, &platform, "Down");
    assert_eq!(
        state.to_settings_view_model().unwrap().selected_field,
        SettingsField::TimeSyncSource
    );
    press(&mut state, &platform, "Enter");

    assert_eq!(
        manager.load_config().unwrap().time_sync.source,
        TimeSyncSource::OperatingSystem
    );
    assert!(
        platform
            .calls()
            .iter()
            .any(|call| matches!(call, MockCall::SystemTime))
    );
    assert!(!state.time_sync_failure_dialog_visible());
}

#[test]
fn malformed_time_server_shows_error_and_does_not_save() {
    let fixture = FixtureRoot::new("malformed-time-server");
    let platform = mock_platform(fixture.path());
    let manager = initialize_profiles(&platform);
    let mut state = start_as_system_user(&platform, "AdminUser");

    open_time_server_editor(&mut state, &platform);
    for character in "not a URL".chars() {
        press(&mut state, &platform, &character.to_string());
    }
    press(&mut state, &platform, "Enter");

    assert_eq!(manager.load_config().unwrap().time_sync.server_url, None);
    assert!(state.time_sync_failure_dialog_visible());
    assert!(
        state
            .time_sync_failure_message()
            .is_some_and(|message| message.contains("setting was not saved"))
    );
}

#[test]
fn editor_settings_save_normalized_explorer_open_suffixes() {
    let fixture = FixtureRoot::new("editor-open-suffixes");
    let platform = mock_platform(fixture.path());
    let manager = initialize_profiles(&platform);
    let mut state = start_as_system_user(&platform, "AdminUser");

    open_settings_from_home(&mut state, &platform);
    select_category(&mut state, &platform, SettingsCategory::Applications);
    press(&mut state, &platform, "Down");
    press(&mut state, &platform, "Enter");
    assert_eq!(
        state.to_settings_view_model().unwrap().selected_field,
        SettingsField::ExplorerOpenExtensions
    );

    press(&mut state, &platform, "Enter");
    let existing_len = state
        .to_settings_view_model()
        .unwrap()
        .file_extensions_editor
        .expect("file suffix editor")
        .value
        .chars()
        .count();
    for _ in 0..existing_len {
        press(&mut state, &platform, "Backspace");
    }
    for character in ".RS, rs; .d.ts".chars() {
        press(&mut state, &platform, &character.to_string());
    }
    press(&mut state, &platform, "Enter");

    assert_eq!(
        manager
            .load_config()
            .unwrap()
            .editor
            .explorer_open_extensions,
        vec!["rs".to_string(), "d.ts".to_string()]
    );
    assert!(
        state
            .to_settings_view_model()
            .unwrap()
            .file_extensions_editor
            .is_none()
    );
}

#[test]
fn appearance_changes_apply_only_to_the_current_system_user() {
    let fixture = FixtureRoot::new("user-permissions");
    let platform = mock_platform(fixture.path());
    let manager = initialize_profiles(&platform);
    let mut state = start_as_system_user(&platform, "NormalUser");

    open_settings_from_home(&mut state, &platform);
    let appearance = state.to_settings_view_model().unwrap();
    assert!(appearance.locked_message.is_none());
    assert!(
        appearance
            .cards
            .iter()
            .flat_map(|card| &card.items)
            .all(|item| item.enabled)
    );

    press(&mut state, &platform, "Down");
    press(&mut state, &platform, "Enter");
    assert_eq!(
        manager
            .load_profile("NormalUser")
            .unwrap()
            .unwrap()
            .appearance
            .border_shape,
        BorderShape::Square
    );
    assert_eq!(
        manager
            .load_profile("AdminUser")
            .unwrap()
            .unwrap()
            .appearance
            .border_shape,
        BorderShape::Rounded
    );
}

#[test]
fn advanced_animation_phases_save_per_user_and_restore_with_speed_defaults() {
    let fixture = FixtureRoot::new("animation-phases");
    let platform = mock_platform(fixture.path());
    let manager = initialize_profiles(&platform);
    let mut state = start_as_system_user(&platform, "NormalUser");
    open_settings_from_home(&mut state, &platform);
    for _ in 0..5 {
        press(&mut state, &platform, "Down");
    }
    press(&mut state, &platform, "Enter");
    adjust_number(&mut state, &platform, true);
    press(&mut state, &platform, "Enter");
    press(&mut state, &platform, "Down");
    let phases = [
        SettingsField::PageAcceleration,
        SettingsField::PageCruise,
        SettingsField::PageDeceleration,
        SettingsField::DialogAcceleration,
        SettingsField::DialogCruise,
        SettingsField::DialogDeceleration,
        SettingsField::MenuAcceleration,
        SettingsField::MenuCruise,
        SettingsField::MenuDeceleration,
    ];
    let model = state.to_settings_view_model().unwrap();
    assert_eq!(model.selected_field, SettingsField::AnimationAdvanced);
    assert!(
        !model
            .cards
            .iter()
            .flat_map(|card| &card.items)
            .any(|item| phases.contains(&item.field))
    );
    press(&mut state, &platform, "Enter");
    for (index, value) in [56, 111, 56, 46, 91, 46, 41, 81, 41]
        .into_iter()
        .enumerate()
    {
        let model = state.to_settings_view_model().unwrap();
        assert_eq!(model.animation_editor.as_ref().unwrap().selected, index);
        assert!(model.number_editor.is_none());
        press(&mut state, &platform, "Ctrl+A");
        state.apply_input_with_platform(InputEvent::Paste(value.to_string()), &platform);
        press(&mut state, &platform, "Tab");
    }
    assert_eq!(
        manager
            .load_profile("NormalUser")
            .unwrap()
            .unwrap()
            .appearance
            .animation_timings,
        storage::AnimationTimingsConfig::default()
    );
    press(&mut state, &platform, "F10");
    let model = state.to_settings_view_model().unwrap();
    assert!(model.animation_editor.is_none());
    assert_eq!(model.selected_field, SettingsField::AnimationAdvanced);
    assert_eq!(state.active_screen(), ShellScreen::Settings);
    let saved = &manager
        .load_profile("NormalUser")
        .unwrap()
        .unwrap()
        .appearance;
    assert_eq!(saved.animation_speed_percent, 125);
    assert_eq!(saved.animation_timings.page, [56, 111, 56]);
    assert_eq!(saved.animation_timings.dialog, [46, 91, 46]);
    assert_eq!(saved.animation_timings.menu, [41, 81, 41]);
    assert_eq!(
        manager
            .load_profile("AdminUser")
            .unwrap()
            .unwrap()
            .appearance
            .animation_timings,
        storage::AnimationTimingsConfig::default()
    );

    let mut reopened = start_as_system_user(&platform, "NormalUser");
    open_settings_from_home(&mut reopened, &platform);
    press(&mut reopened, &platform, "Home");
    for _ in 0..4 {
        press(&mut reopened, &platform, "Down");
    }
    press(&mut reopened, &platform, "Enter");
    press(&mut reopened, &platform, "End");
    press(&mut reopened, &platform, "Up");
    assert_eq!(
        reopened.to_settings_view_model().unwrap().selected_field,
        SettingsField::ResetAnimationSpeed
    );
    press(&mut reopened, &platform, "Enter");
    let restored = &manager
        .load_profile("NormalUser")
        .unwrap()
        .unwrap()
        .appearance;
    assert_eq!(restored.animation_speed_percent, 100);
    assert_eq!(
        restored.animation_timings,
        storage::AnimationTimingsConfig::default()
    );
    assert_eq!(restored.motion_preference, MotionPreference::Reduced);
}

#[test]
fn animation_form_cancels_drafts_and_validates_all_fields_before_saving() {
    let fixture = FixtureRoot::new("animation-form");
    let platform = mock_platform(fixture.path());
    let manager = initialize_profiles(&platform);
    let mut state = start_as_system_user(&platform, "NormalUser");
    open_settings_from_home(&mut state, &platform);
    for _ in 0..6 {
        press(&mut state, &platform, "Down");
    }
    press(&mut state, &platform, "Enter");
    let original = manager.load_profile("NormalUser").unwrap();
    press(&mut state, &platform, "Ctrl+A");
    state.apply_input_with_platform(InputEvent::Paste("123".into()), &platform);
    press(&mut state, &platform, "Esc");
    assert!(
        state
            .to_settings_view_model()
            .unwrap()
            .animation_editor
            .is_none()
    );
    assert_eq!(manager.load_profile("NormalUser").unwrap(), original);
    press(&mut state, &platform, "Enter");
    press(&mut state, &platform, "Ctrl+A");
    state.apply_input_with_platform(InputEvent::Paste("123".into()), &platform);
    press(&mut state, &platform, "Tab");
    press(&mut state, &platform, "Ctrl+A");
    state.apply_input_with_platform(InputEvent::Paste("99999".into()), &platform);
    press(&mut state, &platform, "F10");
    let model = state.to_settings_view_model().unwrap();
    let editor = model.animation_editor.as_ref().unwrap();
    assert_eq!(editor.selected, 1);
    assert!(!editor.message.is_empty());
    assert_eq!(manager.load_profile("NormalUser").unwrap(), original);
    let main = match ui::compute_shell_layout(Rect::new(0, 0, 120, 40)) {
        ui::ShellLayout::Full { main, .. } | ui::ShellLayout::Compact(main) => main,
    };
    let layout = editor.layout(main);
    let reset = layout
        .form_buttons
        .iter()
        .find(|(index, _)| editor.buttons[*index].id == "reset")
        .unwrap()
        .1;
    state.apply_input_with_platform(
        InputEvent::mouse_down(ui::MouseButton::Left, (reset.x, reset.y)),
        &platform,
    );
    assert_eq!(
        state
            .to_settings_view_model()
            .unwrap()
            .animation_editor
            .unwrap()
            .fields[0]
            .value,
        "123"
    );
    state.apply_input_with_platform(
        InputEvent::mouse_up(ui::MouseButton::Left, (reset.x, reset.y)),
        &platform,
    );
    let draft = state
        .to_settings_view_model()
        .unwrap()
        .animation_editor
        .unwrap();
    let defaults = storage::AnimationTimingsConfig::default();
    let expected = defaults
        .page
        .into_iter()
        .chain(defaults.dialog)
        .chain(defaults.menu)
        .map(|phase| phase.to_string())
        .collect::<Vec<_>>();
    assert_eq!(
        draft
            .fields
            .iter()
            .map(|field| field.value.clone())
            .collect::<Vec<_>>(),
        expected
    );
    assert_eq!(manager.load_profile("NormalUser").unwrap(), original);
    press(&mut state, &platform, "Esc");
    assert_eq!(
        state.to_settings_view_model().unwrap().selected_field,
        SettingsField::AnimationAdvanced
    );
    assert_eq!(state.active_screen(), ShellScreen::Settings);
}

#[test]
fn animation_speed_is_per_user_adjustable_and_has_a_default_button() {
    let fixture = FixtureRoot::new("animation-speed");
    let platform = mock_platform(fixture.path());
    let manager = initialize_profiles(&platform);
    let mut state = start_as_system_user(&platform, "NormalUser");

    open_settings_from_home(&mut state, &platform);
    for _ in 0..5 {
        press(&mut state, &platform, "Down");
    }
    let model = state.to_settings_view_model().unwrap();
    assert_eq!(model.selected_field, SettingsField::AnimationSpeed);
    let speed = model
        .cards
        .iter()
        .flat_map(|card| &card.items)
        .find(|item| item.field == SettingsField::AnimationSpeed)
        .expect("animation speed setting");
    assert_eq!(speed.value, "100%");
    assert_eq!(speed.kind, ui::SettingsControlKind::Stepper);
    assert!(speed.enabled);

    press(&mut state, &platform, "Enter");
    adjust_number(&mut state, &platform, true);
    press(&mut state, &platform, "Enter");
    let normal_appearance = || {
        manager
            .load_profile("NormalUser")
            .unwrap()
            .unwrap()
            .appearance
    };
    assert_eq!(normal_appearance().animation_speed_percent, 125);

    let terminal_area = Rect::new(0, 0, 120, 40);
    let ui::ShellLayout::Full { main, .. } = ui::compute_shell_layout(terminal_area) else {
        panic!("expected full settings layout");
    };
    let speed_area = ui::settings_layout(main, &state.to_settings_view_model().unwrap())
        .fields
        .into_iter()
        .find(|field| field.field == SettingsField::AnimationSpeed)
        .expect("visible animation speed field")
        .area;
    state.apply_input_with_platform(
        InputEvent::mouse_down(ui::MouseButton::Left, (speed_area.x, speed_area.y)),
        &platform,
    );
    let number_model = state.to_settings_view_model().unwrap();
    let editor = number_model
        .number_editor
        .as_ref()
        .expect("animation speed number dialog");
    assert_eq!(editor.input.value(), "125");
    assert_eq!(normal_appearance().animation_speed_percent, 125);
    for key in ["Ctrl+A", "2", "0", "0", "Enter"] {
        press(&mut state, &platform, key);
    }
    assert_eq!(normal_appearance().animation_speed_percent, 200);
    assert!(
        state
            .to_settings_view_model()
            .unwrap()
            .number_editor
            .is_none()
    );
    press(&mut state, &platform, "Enter");
    adjust_number(&mut state, &platform, false);
    press(&mut state, &platform, "Enter");
    assert_eq!(normal_appearance().animation_speed_percent, 175);

    press(&mut state, &platform, "Down");
    assert_eq!(
        state.to_settings_view_model().unwrap().selected_field,
        SettingsField::AnimationAdvanced
    );
    press(&mut state, &platform, "Down");
    assert_eq!(
        state.to_settings_view_model().unwrap().selected_field,
        SettingsField::ResetAnimationSpeed
    );
    press(&mut state, &platform, "Enter");
    assert_eq!(normal_appearance().animation_speed_percent, 100);

    press(&mut state, &platform, "Up");
    press(&mut state, &platform, "Up");
    press(&mut state, &platform, "Up");
    assert_eq!(
        state.to_settings_view_model().unwrap().selected_field,
        SettingsField::MotionPreference
    );
    press(&mut state, &platform, "Enter");
    assert_eq!(
        normal_appearance().motion_preference,
        MotionPreference::Reduced
    );
    let reduced = state.to_settings_view_model().unwrap();
    for field in [SettingsField::AnimationSpeed] {
        assert!(
            !reduced
                .cards
                .iter()
                .flat_map(|card| &card.items)
                .find(|item| item.field == field)
                .expect("animation setting")
                .enabled
        );
    }
    assert!(
        reduced
            .cards
            .iter()
            .flat_map(|card| &card.items)
            .find(|item| item.field == SettingsField::ResetAnimationSpeed)
            .unwrap()
            .enabled
    );
}

#[test]
fn default_theme_icon_picker_persists_ascii_and_image_modes_when_supported() {
    let fixture = FixtureRoot::new("theme-icon-mode-supported");
    let platform = mock_platform(fixture.path()).with_kind(platform::native_platform().kind());
    let manager = initialize_profiles(&platform);
    let saved_mode = || {
        manager
            .load_profile("NormalUser")
            .unwrap()
            .unwrap()
            .appearance
            .icon_display_mode
    };
    let mut state = start_as_system_user(&platform, "NormalUser");
    state.set_terminal_image_support(true);

    open_settings_from_home(&mut state, &platform);
    let images_by_default = IconDisplayMode::default() == IconDisplayMode::Image;
    assert_eq!(state.graphical_icons_enabled(), images_by_default);
    assert_eq!(
        state.to_settings_view_model().unwrap().selected_field,
        SettingsField::Theme
    );

    press(&mut state, &platform, "Enter");
    assert_eq!(
        state
            .to_settings_view_model()
            .unwrap()
            .picker
            .as_ref()
            .map(|picker| picker.kind),
        Some(SettingsPickerKind::Theme)
    );
    press(&mut state, &platform, "Enter");
    let picker = state.to_settings_view_model().unwrap().picker.unwrap();
    assert_eq!(picker.kind, SettingsPickerKind::DefaultThemeIcons);
    assert_eq!(picker.options.len(), 2);
    assert!(picker.options.iter().all(|option| option.enabled));
    assert_eq!(picker.selected_index, usize::from(images_by_default));

    press(&mut state, &platform, "Home");
    press(&mut state, &platform, "Enter");
    assert_eq!(saved_mode(), IconDisplayMode::Ascii);
    assert!(!state.graphical_icons_enabled());

    press(&mut state, &platform, "Enter");
    press(&mut state, &platform, "Enter");
    press(&mut state, &platform, "Down");
    press(&mut state, &platform, "Enter");
    assert_eq!(saved_mode(), IconDisplayMode::Image);
    assert!(state.graphical_icons_enabled());

    for key in ["End", "Enter", "R"] {
        press(&mut state, &platform, key);
    }
    assert_eq!(saved_mode(), IconDisplayMode::default());
    assert_eq!(state.graphical_icons_enabled(), images_by_default);
}

#[test]
fn unsupported_terminal_locks_default_theme_to_ascii_and_disables_image_option() {
    let fixture = FixtureRoot::new("theme-icon-mode-unsupported");
    let platform = mock_platform(fixture.path());
    initialize_profiles(&platform);
    let mut state = start_as_system_user(&platform, "NormalUser");
    state.set_terminal_image_support(false);

    open_settings_from_home(&mut state, &platform);
    let appearance = state.to_settings_view_model().unwrap();
    let theme = appearance
        .cards
        .iter()
        .flat_map(|card| &card.items)
        .find(|item| item.field == SettingsField::Theme)
        .expect("Theme setting");
    assert_eq!(theme.value, "Default theme / ASCII icons");
    assert!(!state.graphical_icons_enabled());

    press(&mut state, &platform, "Enter");
    press(&mut state, &platform, "Enter");
    let picker = state.to_settings_view_model().unwrap().picker.unwrap();
    assert_eq!(picker.kind, SettingsPickerKind::DefaultThemeIcons);
    assert_eq!(picker.selected_index, 0);
    assert!(picker.options[0].enabled);
    assert!(!picker.options[1].enabled);

    press(&mut state, &platform, "Down");
    press(&mut state, &platform, "Enter");
    let model = state.to_settings_view_model().unwrap();
    assert!(model.picker.is_some());
    assert!(model.status.contains("unavailable"));
    assert!(!state.graphical_icons_enabled());
}

fn initialize_profiles(platform: &MockPlatform) -> StorageManager {
    let manager = StorageManager::open_from_platform(platform)
        .unwrap()
        .manager;
    for name in ["AdminUser", "NormalUser"] {
        manager.update_profile(name, |_| {}).unwrap();
    }
    manager
}

fn start_as_system_user(platform: &MockPlatform, username: &str) -> ShellSession {
    let mut startup = prepare_shell_startup(platform).expect("startup");
    let session = startup.system_session.as_mut().unwrap();
    session.username = username.to_owned();
    session.user_id = username.to_owned();
    let state = ShellSession::new_with_startup(default_config(), (120, 40), startup);
    assert_eq!(state.active_screen(), ShellScreen::Home);
    state
}

fn open_settings_from_home(state: &mut ShellSession, platform: &MockPlatform) {
    let target = state
        .to_home_view_model()
        .entries()
        .iter()
        .position(|entry| entry.label == "Settings")
        .expect("Settings home entry");
    while state.selected_home_entry_index() < target {
        press(state, platform, "Right");
    }
    press(state, platform, "Enter");
}

fn open_time_server_editor(state: &mut ShellSession, platform: &MockPlatform) {
    open_settings_from_home(state, platform);
    select_category(state, platform, SettingsCategory::RegionTime);
    for _ in 0..4 {
        press(state, platform, "Down");
    }
    assert_eq!(
        state.to_settings_view_model().unwrap().selected_field,
        SettingsField::TimeSyncServer
    );
    press(state, platform, "Enter");
    assert!(
        state
            .to_settings_view_model()
            .unwrap()
            .time_sync_server_editor
            .is_some()
    );
}

fn press(state: &mut ShellSession, platform: &MockPlatform, key: &str) {
    let input = if key == "Ctrl+A" {
        InputEvent::Key(KeyInput::with_modifiers(
            InputKey::Char('a'),
            InputModifiers::ctrl(),
        ))
    } else {
        InputEvent::from_key_label(key)
    };
    state.apply_input_with_platform(input, platform);
}

fn select_field(state: &mut ShellSession, platform: &MockPlatform, field: SettingsField) {
    press(state, platform, "Home");
    let model = state.to_settings_view_model().unwrap();
    if model.selected_category == SettingsCategory::Applications {
        press(state, platform, "Right");
    }
    let index = model
        .cards
        .iter()
        .flat_map(|card| &card.items)
        .position(|item| item.field == field)
        .unwrap();
    for _ in 0..index {
        press(state, platform, "Down");
    }
    assert_eq!(
        state.to_settings_view_model().unwrap().selected_field,
        field
    );
}

fn mock_platform(base: &Path) -> MockPlatform {
    for directory in [
        "Desktop",
        "Documents",
        "Downloads",
        "Pictures",
        "Videos",
        "Music",
        "Roaming",
        "Local",
        "Temp",
    ] {
        fs::create_dir_all(base.join(directory)).expect("fixture directory");
    }
    MockPlatform::new(user_dirs(base), app_paths(base))
        .with_kind(PlatformKind::Windows)
        .with_capabilities(PlatformCapabilities::native_supported())
}

fn user_dirs(base: &Path) -> UserDirs {
    UserDirs::new(
        base.join("Desktop"),
        base.join("Documents"),
        base.join("Downloads"),
        base.join("Pictures"),
        base.join("Videos"),
        base.join("Music"),
        base.join("Roaming"),
    )
    .expect("fixture user directories")
}

fn app_paths(base: &Path) -> AppPaths {
    build_windows_app_paths(base.join("Roaming"), base.join("Local"), base.join("Temp"))
        .expect("fixture app paths")
}

struct FixtureRoot {
    path: PathBuf,
}

impl FixtureRoot {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "tundra-shell-settings-{name}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir_all(&path).expect("create fixture root");
        Self {
            path: fs::canonicalize(path).expect("canonicalize fixture root"),
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for FixtureRoot {
    fn drop(&mut self) {
        let _ = cleanup_temp_path(&self.path);
    }
}

fn select_category(state: &mut ShellSession, platform: &MockPlatform, category: SettingsCategory) {
    for _ in 0..32 {
        if state.to_settings_view_model().unwrap().focus == ui::SettingsFocus::Categories {
            break;
        }
        press(state, platform, "Shift+Tab");
    }
    for _ in 0..SettingsCategory::ALL.len() {
        if state.to_settings_view_model().unwrap().selected_category == category {
            press(state, platform, "Enter");
            return;
        }
        press(state, platform, "Down");
    }
    panic!("settings category not reachable: {category:?}");
}

#[test]
fn unavailable_device_pages_are_removed_from_settings_on_every_platform() {
    for kind in [
        PlatformKind::Macos,
        PlatformKind::Windows,
        PlatformKind::Linux,
        PlatformKind::Unsupported,
    ] {
        let fixture = FixtureRoot::new(&format!("device-settings-{kind:?}"));
        let platform = mock_platform(fixture.path()).with_kind(kind);
        let manager = initialize_profiles(&platform);
        for username in ["AdminUser", "NormalUser"] {
            let mut state = start_as_system_user(&platform, username);
            open_settings_from_home(&mut state, &platform);
            let before = manager.load_config().unwrap();
            let users_before = manager.load_profile(username).unwrap();
            for expected in SettingsCategory::ALL {
                select_category(&mut state, &platform, expected);
                let category = state.to_settings_view_model().unwrap().selected_category;
                assert!(!matches!(
                    category,
                    SettingsCategory::Sound
                        | SettingsCategory::Display
                        | SettingsCategory::Wifi
                        | SettingsCategory::Bluetooth
                ));
            }
            assert_eq!(manager.load_config().unwrap(), before);
            assert_eq!(manager.load_profile(username).unwrap(), users_before);
        }
    }
}

#[test]
fn picker_search_edits_at_the_visible_cursor() {
    let fixture = FixtureRoot::new("picker-text-cursor");
    let platform = mock_platform(fixture.path());
    initialize_profiles(&platform);
    let mut state = start_as_system_user(&platform, "AdminUser");
    open_settings_from_home(&mut state, &platform);
    select_category(&mut state, &platform, SettingsCategory::RegionTime);
    select_field(&mut state, &platform, SettingsField::Language);
    for key in ["Enter", "E", "n", "g", "Left", "X"] {
        press(&mut state, &platform, key);
    }
    let picker = state.to_settings_view_model().unwrap().picker.unwrap();
    assert_eq!(picker.query, "EnXg");
    assert_eq!(picker.editing.input("query", &picker.query).cursor(), 3);
    press(&mut state, &platform, "Backspace");
    assert_eq!(
        state
            .to_settings_view_model()
            .unwrap()
            .picker
            .unwrap()
            .query,
        "Eng"
    );
    press(&mut state, &platform, "Esc");
}

fn adjust_number(state: &mut ShellSession, platform: &MockPlatform, increase: bool) {
    let model = state.to_settings_view_model().unwrap();
    let (width, height) = state.terminal_size();
    let main = match ui::compute_shell_layout(Rect::new(0, 0, width, height)) {
        ui::ShellLayout::Full { main, .. } | ui::ShellLayout::Compact(main) => main,
    };
    let layout = ui::settings_layout(main, &model);
    let number = layout.number_editor.unwrap();
    let area = if increase {
        number.increase
    } else {
        number.decrease
    };
    let point = (area.x, area.y);
    state.apply_input_with_platform(
        InputEvent::mouse_down(ui::MouseButton::Left, point),
        platform,
    );
    state.apply_input_with_platform(InputEvent::mouse_up(ui::MouseButton::Left, point), platform);
}
