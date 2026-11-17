use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use chrono::Utc;
use platform::mock::MockPlatform;
use platform::{Platform, PlatformCapabilities, PlatformKind, UserDirs, build_windows_app_paths};
use shell::{
    HomeModeOverride, InputEvent, InputKey, InputModifiers, InputPhase, KeyInput, PointerButton,
    ShellComponent, ShellLaunchConfig, ShellScreen, ShellSession, prepare_shell_startup,
};
use storage::{BorderColor, BorderShape, ClockEntryRecord, ClockProfile};

fn default_config() -> ShellLaunchConfig {
    ShellLaunchConfig {
        home_mode_override: HomeModeOverride::BuildDefault,
    }
}

#[test]
fn system_user_starts_with_new_preferences_and_returns_directly_to_home() {
    for kind in [PlatformKind::Macos, PlatformKind::Windows] {
        let fixture = FixtureRoot::new(kind.as_str());
        let username = "系统用户";
        let platform = mock_platform(fixture.path())
            .with_kind(kind)
            .with_current_username(username);
        let data = platform.app_paths().unwrap().data_path().to_path_buf();
        fs::create_dir_all(&data).unwrap();
        let old_accounts = data.join("users.v2.json");
        fs::write(&old_accounts, r#"{"schema_version":99}"#).unwrap();
        let old_clock = data.join("clock.v1.json");
        fs::write(&old_clock, r#"{"schema_version":99}"#).unwrap();

        let startup = prepare_shell_startup(&platform).unwrap();
        let manager = startup.storage_manager.clone().unwrap();
        let mut state = ShellSession::new_with_startup(default_config(), (108, 20), startup);
        assert_eq!(state.active_screen(), ShellScreen::FirstRunSetup);
        assert_eq!(state.to_setup_view_model().step, ui::SetupStep::Language);
        assert!(manager.load_profile(username).unwrap().is_none());
        assert!(manager.load_clock().unwrap().profiles.is_empty());

        state.apply_input_with_platform(function_key(9), &platform);
        assert_eq!(state.active_screen(), ShellScreen::Home);
        assert_eq!(state.auth_session().unwrap().username, username);
        assert!(manager.load_profile(username).unwrap().is_some());
        assert!(state.to_home_view_model().entries().iter().all(|entry| {
            !matches!(
                entry.icon_identity(),
                "users" | "user_profile" | "user_management"
            )
        }));
        let session = state.auth_session().cloned();
        for key in ["l", "Ctrl+L"] {
            state.apply_input_with_platform(InputEvent::from_key_label(key), &platform);
            assert_eq!(state.active_screen(), ShellScreen::Home);
            assert_eq!(state.auth_session(), session.as_ref());
        }

        let startup = prepare_shell_startup(&platform).unwrap();
        let reopened = ShellSession::new_with_startup(default_config(), (108, 20), startup);
        assert_eq!(reopened.active_screen(), ShellScreen::Home);
        assert_eq!(reopened.auth_session().unwrap().username, username);
        assert_eq!(
            fs::read(&old_accounts).unwrap(),
            br#"{"schema_version":99}"#
        );
        assert_eq!(fs::read(&old_clock).unwrap(), br#"{"schema_version":99}"#);
    }
}

#[cfg(target_os = "macos")]
#[test]
fn macos_uses_the_native_process_username() {
    let expected = std::process::Command::new("/usr/bin/id")
        .arg("-un")
        .output()
        .unwrap();
    assert!(expected.status.success());
    let platform = platform::native_platform();
    let session = identity::AuthSession::current(platform.as_ref()).unwrap();
    assert_eq!(
        session.username,
        String::from_utf8(expected.stdout).unwrap().trim_end()
    );
}

#[test]
fn appearance_setup_validates_custom_colors_and_persists_all_choices() {
    let fixture = FixtureRoot::new("first-run-appearance");
    let platform = mock_platform(fixture.path());
    let startup = prepare_shell_startup(&platform).expect("startup");
    let manager = startup.storage_manager.clone().expect("storage manager");
    let mut state = ShellSession::new_with_startup(default_config(), (120, 40), startup);

    advance_to_appearance(&mut state);
    assert_eq!(
        setup_hit_components(&state),
        vec![
            ShellComponent::SetupAppearanceShape,
            ShellComponent::SetupAppearanceThemeColor,
            ShellComponent::SetupAppearanceThemeCustom,
            ShellComponent::SetupAppearanceAccentColor,
            ShellComponent::SetupAppearanceAccentCustom,
            ShellComponent::SetupAppearanceSubmit,
        ]
    );

    state.apply_input(InputEvent::from_key_label("Right"));
    assert_eq!(
        state.to_setup_view_model().border_shape,
        ui::BorderShape::Square
    );

    state.apply_input(InputEvent::from_key_label("Tab"));
    state.apply_input(InputEvent::from_key_label("Tab"));
    state.apply_input(InputEvent::from_key_label("Enter"));
    assert_eq!(
        state.focused_component(),
        ShellComponent::SetupCustomColorDialog
    );
    clear_setup_custom_color(&mut state);
    type_text(&mut state, "#38BDF8");
    assert!(state.to_setup_view_model().custom_color_valid);
    state.apply_input(InputEvent::from_key_label("Enter"));
    assert_eq!(state.to_setup_view_model().theme_color_value, "#38BDF8");

    state.apply_input(InputEvent::from_key_label("Tab"));
    state.apply_input(InputEvent::from_key_label("Tab"));
    state.apply_input(InputEvent::from_key_label("Enter"));
    clear_setup_custom_color(&mut state);
    type_text(&mut state, "#12GG00");
    assert!(!state.to_setup_view_model().custom_color_valid);
    state.apply_input(InputEvent::from_key_label("Enter"));
    assert_eq!(
        state.focused_component(),
        ShellComponent::SetupCustomColorDialog
    );
    assert_eq!(
        state.to_setup_view_model().custom_color_error.as_deref(),
        Some("Invalid color. Use #RRGGBB or a supported color name.")
    );
    clear_setup_custom_color(&mut state);
    type_text(&mut state, "#AABBCC");
    state.apply_input(InputEvent::from_key_label("Enter"));
    state.apply_input(InputEvent::from_key_label("Tab"));
    assert_eq!(
        state.focused_component(),
        ShellComponent::SetupAppearanceSubmit
    );
    state.apply_input(InputEvent::from_key_label("Enter"));

    assert_eq!(state.active_screen(), ShellScreen::Home);
    let profile = manager.load_profile("operator").unwrap().unwrap();
    assert_eq!(profile.appearance.border_shape, BorderShape::Square);
    assert_eq!(
        profile.appearance.border_color,
        BorderColor::Rgb(0x38, 0xBD, 0xF8)
    );
    assert_eq!(
        profile.appearance.accent_color,
        BorderColor::Rgb(0xAA, 0xBB, 0xCC)
    );
}

#[test]
fn appearance_setup_prevents_matching_theme_and_accent_colors() {
    let (_fixture, mut state) = fresh_setup_state("first-run-distinct-colors");

    advance_to_appearance(&mut state);

    state.apply_input(InputEvent::from_key_label("Tab"));
    assert_eq!(
        state.focused_component(),
        ShellComponent::SetupAppearanceThemeColor
    );
    state.apply_input(InputEvent::from_key_label("Tab"));
    assert_eq!(
        state.focused_component(),
        ShellComponent::SetupAppearanceThemeCustom
    );
    state.apply_input(InputEvent::from_key_label("Enter"));
    assert_eq!(
        state.focused_component(),
        ShellComponent::SetupCustomColorDialog
    );
    clear_setup_custom_color(&mut state);
    type_text(&mut state, "#38BDF8");
    state.apply_input(InputEvent::from_key_label("Enter"));
    assert_eq!(state.to_setup_view_model().theme_color_value, "#38BDF8");

    state.apply_input(InputEvent::from_key_label("Tab"));
    state.apply_input(InputEvent::from_key_label("Tab"));
    assert_eq!(
        state.focused_component(),
        ShellComponent::SetupAppearanceAccentCustom
    );
    state.apply_input(InputEvent::from_key_label("Enter"));
    clear_setup_custom_color(&mut state);
    type_text(&mut state, "#38BDF8");
    let model = state.to_setup_view_model();
    assert!(model.custom_color_conflicts_with_theme);
    assert!(!model.custom_color_valid);

    state.apply_input(account_modified_key(InputKey::Enter, InputModifiers::CTRL));
    assert_eq!(
        state.focused_component(),
        ShellComponent::SetupCustomColorDialog
    );
    assert_eq!(
        state.to_setup_view_model().custom_color_error.as_deref(),
        Some("Accent color must be different from the theme color.")
    );
}

#[test]
fn clock_keyboard_flow_creates_manages_and_persists_entries() {
    let fixture = FixtureRoot::new("clock-keyboard-flow");
    let platform = mock_platform(fixture.path());
    let startup = prepare_shell_startup(&platform).expect("startup");
    let manager = startup.storage_manager.clone().expect("storage manager");
    let mut state = ShellSession::new_with_startup(default_config(), (120, 40), startup);
    state.apply_input(function_key(9));

    open_clock(&mut state);
    let new_button = component_center(&state, ShellComponent::ClockNewButton);
    state.apply_input(InputEvent::mouse_down(PointerButton::Left, new_button));
    assert_eq!(state.focused_component(), ShellComponent::ClockCreateInput);
    state.apply_input(InputEvent::from_key_label("Esc"));

    create_clock_entry(&mut state, "07 30 00", false);
    assert_eq!(state.to_clock_view_model().alarms.len(), 1);
    assert!(
        state.to_clock_view_model().alarms[0]
            .label
            .contains("07:30:00")
    );
    let alarm_row = component_center(&state, ShellComponent::ClockEntryList);
    state.apply_input(InputEvent::mouse_down(PointerButton::Left, alarm_row));
    assert_eq!(
        state
            .to_notification_view_model()
            .map(|notification| notification.title),
        Some("Manage Alarm".to_string())
    );
    state.apply_input(InputEvent::from_key_label("Esc"));

    create_clock_entry(&mut state, "00 00 05", true);
    assert_eq!(state.to_clock_view_model().countdowns.len(), 1);
    assert_eq!(state.focused_component(), ShellComponent::ClockEntryList);

    state.apply_input(InputEvent::from_key_label("Enter"));
    assert_eq!(
        state
            .to_notification_view_model()
            .map(|notification| notification.title),
        Some("Manage Countdown".to_string())
    );
    state.apply_input(InputEvent::from_key_label("t"));
    assert!(state.to_clock_view_model().countdowns[0].strong);

    state.apply_input(InputEvent::from_key_label("Enter"));
    state.apply_input(InputEvent::from_key_label("x"));
    assert!(state.to_clock_view_model().countdowns.is_empty());

    let username = &state.auth_session().unwrap().username;
    let document = manager.load_clock().expect("clock document");
    let profile = document.profiles.get(username).expect("user clock profile");
    assert_eq!(profile.entries.len(), 1);
    assert!(matches!(
        profile.entries[0],
        ClockEntryRecord::DailyAlarm { .. }
    ));
}

#[test]
fn expired_countdown_waits_for_initial_sync_then_is_delivered_and_removed() {
    let fixture = FixtureRoot::new("clock-expired-on-login");
    let platform = mock_platform(fixture.path());

    let startup = prepare_shell_startup(&platform).expect("startup");
    let manager = startup.storage_manager.clone().expect("storage manager");
    let username = startup.system_session.as_ref().unwrap().username.clone();
    manager.update_profile(&username, |_| {}).unwrap();
    let mut document = manager.load_clock().expect("clock document");
    document.profiles.insert(
        username.clone(),
        ClockProfile {
            next_id: 2,
            entries: vec![ClockEntryRecord::Countdown {
                id: 1,
                deadline_epoch_ms: 1,
                strong: false,
            }],
        },
    );
    manager.save_clock(&document).expect("clock saved");

    let mut state = ShellSession::new_with_startup(default_config(), (120, 40), startup);

    assert_eq!(
        state.to_shell_chrome_view_model().status.toast.as_deref(),
        Some("Waiting for initial time sync to restore reminders")
    );
    assert_eq!(
        manager
            .load_clock()
            .expect("clock remains pending")
            .profiles
            .get(&username)
            .expect("profile retained")
            .entries
            .len(),
        1
    );

    state.apply_time_sync_utc_for_test(Utc::now());

    assert!(state.to_clock_view_model().countdowns.is_empty());
    assert_eq!(
        state.to_shell_chrome_view_model().status.toast.as_deref(),
        Some("Countdown finished")
    );
    assert!(
        manager
            .load_clock()
            .expect("clock reloaded")
            .profiles
            .get(&username)
            .expect("profile retained")
            .entries
            .is_empty()
    );
}

fn open_clock(state: &mut ShellSession) {
    assert_eq!(state.active_screen(), ShellScreen::Home);
    state.apply_input(InputEvent::from_key_label("Tab"));
    assert_eq!(state.focused_component(), ShellComponent::HomeEdit);
    state.apply_input(InputEvent::from_key_label("Tab"));
    assert_eq!(state.focused_component(), ShellComponent::ClockButton);
    state.apply_input(InputEvent::from_key_label("Enter"));
    assert_eq!(state.active_screen(), ShellScreen::Clock);
    assert_eq!(state.focused_component(), ShellComponent::ClockNewButton);
}

fn create_clock_entry(state: &mut ShellSession, input: &str, countdown: bool) {
    state.apply_input(InputEvent::from_key_label("n"));
    assert_eq!(state.focused_component(), ShellComponent::ClockCreateInput);
    for (index, digits) in input.split_whitespace().enumerate() {
        if index > 0 {
            state.apply_input(InputEvent::from_key_label("Down"));
        }
        type_text(state, digits);
    }
    state.apply_input(InputEvent::from_key_label("Tab"));
    if countdown {
        state.apply_input(InputEvent::from_key_label("Tab"));
    }
    state.apply_input(InputEvent::from_key_label("Enter"));
    assert_eq!(state.focused_component(), ShellComponent::ClockEntryList);
}

fn component_center(state: &ShellSession, component: ShellComponent) -> (u16, u16) {
    rect_center(component_area(state, component))
}

fn component_area(state: &ShellSession, component: ShellComponent) -> ratatui::layout::Rect {
    state
        .hit_map()
        .regions()
        .iter()
        .rev()
        .find(|region| region.component == component)
        .unwrap_or_else(|| panic!("missing {component:?} hit region"))
        .area
}

fn function_key(number: u8) -> InputEvent {
    InputEvent::Key(KeyInput::with_phase(
        InputKey::F(number),
        InputModifiers::none(),
        InputPhase::Press,
    ))
}

fn account_modified_key(key: InputKey, modifiers: InputModifiers) -> InputEvent {
    InputEvent::Key(KeyInput::with_modifiers(key, modifiers))
}

fn type_text(state: &mut ShellSession, text: &str) {
    for character in text.chars() {
        state.apply_input(InputEvent::from_key_label(character.to_string()));
    }
}

fn clear_setup_custom_color(state: &mut ShellSession) {
    let length = state
        .to_setup_view_model()
        .custom_color_input
        .chars()
        .count();
    for _ in 0..length {
        state.apply_input(InputEvent::from_key_label("Backspace"));
    }
}

fn rect_center(area: ratatui::layout::Rect) -> (u16, u16) {
    (
        area.x.saturating_add(area.width / 2),
        area.y.saturating_add(area.height / 2),
    )
}

fn fresh_setup_state(case: &str) -> (FixtureRoot, ShellSession) {
    let fixture = FixtureRoot::new(case);
    let platform = mock_platform(fixture.path());
    let startup = prepare_shell_startup(&platform).expect("startup");
    let state = ShellSession::new_with_startup(default_config(), (120, 40), startup);
    (fixture, state)
}

fn setup_hit_components(state: &ShellSession) -> Vec<ShellComponent> {
    state
        .hit_map()
        .regions()
        .iter()
        .map(|region| region.component)
        .filter(|component| {
            matches!(
                component,
                ShellComponent::SetupAppearanceShape
                    | ShellComponent::SetupAppearanceThemeColor
                    | ShellComponent::SetupAppearanceThemeCustom
                    | ShellComponent::SetupAppearanceAccentColor
                    | ShellComponent::SetupAppearanceAccentCustom
                    | ShellComponent::SetupAppearanceSubmit
            )
        })
        .collect()
}

fn advance_to_appearance(state: &mut ShellSession) {
    state.apply_input(InputEvent::from_key_label("Enter"));
    assert_eq!(state.to_setup_view_model().step, ui::SetupStep::Timezone);
    state.apply_input(InputEvent::from_key_label("Enter"));
    assert_eq!(state.to_setup_view_model().step, ui::SetupStep::Appearance);
}

fn mock_platform(base: &Path) -> MockPlatform {
    let app_paths =
        build_windows_app_paths(base.join("Roaming"), base.join("Local"), base.join("Temp"))
            .expect("fixture app paths should resolve");
    MockPlatform::new(user_dirs(base), app_paths)
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
    .expect("fixture user directories should resolve")
}

struct FixtureRoot {
    path: PathBuf,
}

impl FixtureRoot {
    fn new(case: &str) -> Self {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "tundra-shell-phase5-{}-{nanos}-{case}",
            std::process::id()
        ));
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for FixtureRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}
