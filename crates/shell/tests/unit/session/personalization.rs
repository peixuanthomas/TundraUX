use super::*;

struct PersonalizationTempGuard(PathBuf);

impl Drop for PersonalizationTempGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fixture(pending: bool) -> (PersonalizationTempGuard, StorageManager, AuthSession) {
    static NEXT_FIXTURE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let sequence = NEXT_FIXTURE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let root = std::env::temp_dir().join(format!(
        "tundra-personalization-{sequence}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let paths = platform::build_linux_app_paths(
        root.join("config"),
        root.join("data"),
        root.join("cache"),
        root.join("state"),
        root.join("temp"),
    )
    .unwrap();
    let manager = StorageManager::open(paths).unwrap().manager;
    let session = AuthSession {
        source: identity::IdentitySource::LinuxCurrentProcess,
        session_id: "linux-session".into(),
        user_id: "linux-uid-1000".into(),
        username: "peixuan".into(),
        role: UserRole::Admin,
        started_at_epoch_ms: 1,
    };
    if !pending {
        manager.update_profile(&session.username, |_| {}).unwrap();
    }
    (PersonalizationTempGuard(root), manager, session)
}

fn copy_assets(source: &std::path::Path, destination: &std::path::Path) {
    std::fs::create_dir_all(destination).unwrap();
    for entry in std::fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let target = destination.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_assets(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn linux_state(manager: &StorageManager, images: bool) -> ShellSession {
    let mut startup = ShellStartupState::clean(
        PlatformKind::Linux,
        PlatformCapabilities::native_supported(),
    );
    startup.storage_manager = Some(manager.clone());
    // Keep language loading and resource repair isolated from other fixtures.
    let assets = manager
        .layout()
        .config_path
        .parent()
        .unwrap()
        .join("test-assets");
    copy_assets(
        std::path::Path::new(ascii_assets::CANONICAL_ASSETS_DIR),
        &assets,
    );
    let (store, _) = ui::AsciiAssetStore::load_default_with_root_and_recovery(&assets).unwrap();
    let mut state = ShellSession::new_with_runtime_services(
        ShellLaunchConfig::default(),
        (120, 40),
        startup,
        ui::RuntimeAsciiAssets::from_store(store),
        ShellRuntimeServices {
            window_owner: Default::default(),
            inherited_auth: None,
            session_snapshot: None,
            language: None,
            terminal_image_support: images,
            explorer: None,
            diagnostics: None,
            editor: ShellEditorTaskRuntime::unavailable(),
            settings: ShellSettingsTaskRuntime::unavailable(),
        },
        None,
    );
    // Resource normalization can report a startup repair; acknowledge it just
    // as the terminal user does before entering the onboarding flow.
    if state.app.notification_center().active_modal().is_some() {
        state.apply_input(InputEvent::from_key_label("Enter"));
    }
    state
}

fn advance_to_appearance(state: &mut ShellSession) {
    assert_eq!(state.to_setup_view_model().step, ui::SetupStep::Language);
    assert_eq!(state.focused_component(), ShellComponent::SetupLanguage);
    state.apply_input(InputEvent::from_key_label("Down"));
    state.apply_input(InputEvent::from_key_label("Enter"));
    assert_eq!(state.to_setup_view_model().step, ui::SetupStep::Appearance);
    assert_eq!(
        state.focused_component(),
        ShellComponent::SetupAppearanceShape
    );
    assert!(state.auth_session().is_none());
}

fn submit(state: &mut ShellSession) {
    for _ in 0..5 {
        state.apply_input(InputEvent::from_key_label("Tab"));
    }
    assert_eq!(
        state.focused_component(),
        ShellComponent::SetupAppearanceSubmit
    );
    state.apply_input(InputEvent::from_key_label("Enter"));
}

#[test]
fn linux_first_login_requires_personalization_and_persists_terminal_safe_choices() {
    for images in [false, true] {
        let (_guard, manager, session) = fixture(true);
        let mut expected_config = manager.load_config().unwrap();
        let mut state = linux_state(&manager, images);
        assert!(!state.graphical_icons_enabled());
        state.attach_system_session(session.clone());
        assert_eq!(state.active_screen(), ShellScreen::FirstRunSetup);
        advance_to_appearance(&mut state);
        expected_config.language = state.selected_setup_language_value();
        assert!(state.auth_session().is_none());
        assert!(state.app.active_appearance().is_none());
        assert!(state.clock_scheduler.is_none());
        // Home shortcuts cannot bypass setup. Appearance input uses existing routing.
        state.apply_input(InputEvent::from_key_label("e"));
        assert_eq!(state.active_screen(), ShellScreen::FirstRunSetup);
        state.apply_input(InputEvent::from_key_label("Right"));
        let chosen_shape = state.pages.setup.setup_border_shape;
        submit(&mut state);
        assert_eq!(state.active_screen(), ShellScreen::Home);
        assert_eq!(state.auth_session(), Some(&session));
        assert!(state.pending_personalization_session.is_none());
        let record = manager.load_profile(&session.username).unwrap().unwrap();
        assert_eq!(record.appearance.border_shape, chosen_shape);
        assert_eq!(
            record.appearance.icon_display_mode,
            storage::IconDisplayMode::Ascii
        );
        assert_eq!(state.app.active_appearance(), Some(&record.appearance));
        assert!(!state.graphical_icons_enabled());
        assert_eq!(manager.load_config().unwrap(), expected_config);
        // Completion survives a new shell process/session.
        let mut reopened = linux_state(&manager, images);
        reopened.attach_system_session(session);
        assert_eq!(reopened.active_screen(), ShellScreen::Home);
        assert!(!reopened.graphical_icons_enabled());
    }
}

#[test]
fn interrupted_linux_personalization_resumes_and_existing_profiles_skip_it() {
    let (_guard, manager, session) = fixture(true);
    let mut state = linux_state(&manager, false);
    state.attach_system_session(session.clone());
    drop(state);
    assert!(manager.load_profile("peixuan").unwrap().is_none());
    let mut reopened = linux_state(&manager, false);
    reopened.attach_system_session(session.clone());
    assert_eq!(reopened.active_screen(), ShellScreen::FirstRunSetup);
    // A saved profile for another username with the same UID is not inherited.
    manager.update_profile(&session.username, |_| {}).unwrap();
    let mut renamed = linux_state(&manager, false);
    let mut session = session;
    session.username = "renamed-user".into();
    renamed.attach_system_session(session);
    assert_eq!(renamed.active_screen(), ShellScreen::FirstRunSetup);
    assert!(renamed.auth_session().is_none());
}

#[test]
fn save_failure_keeps_linux_personalization_pending_and_allows_retry() {
    let (_guard, manager, session) = fixture(true);
    let mut state = linux_state(&manager, false);
    state.attach_system_session(session);
    advance_to_appearance(&mut state);
    let profiles_path = &manager.layout().profiles_path;
    let backup = profiles_path.with_extension("test-backup");
    std::fs::rename(profiles_path, &backup).unwrap();
    std::fs::create_dir(profiles_path).unwrap();
    submit(&mut state);
    assert_eq!(state.active_screen(), ShellScreen::FirstRunSetup);
    assert!(state.auth_session().is_none());
    assert!(state.pending_personalization_session.is_some());
    assert!(state.to_setup_view_model().error.is_some());
    std::fs::remove_dir(profiles_path).unwrap();
    std::fs::rename(backup, profiles_path).unwrap();
    assert!(manager.load_profile("peixuan").unwrap().is_none());
    state.apply_input(InputEvent::from_key_label("Enter"));
    assert_eq!(state.active_screen(), ShellScreen::Home);
    assert!(manager.load_profile("peixuan").unwrap().is_some());
}

#[test]
fn config_save_failure_keeps_linux_setup_pending_and_allows_retry() {
    let (_guard, manager, session) = fixture(true);
    let mut state = linux_state(&manager, false);
    state.attach_system_session(session);
    advance_to_appearance(&mut state);
    let config_path = &manager.layout().config_path;
    let backup = config_path.with_extension("test-backup");
    std::fs::rename(config_path, &backup).unwrap();
    std::fs::create_dir(config_path).unwrap();
    let saved_timezone = state.app.storage_config().timezone.clone();
    submit(&mut state);
    assert_eq!(state.active_screen(), ShellScreen::FirstRunSetup);
    assert!(state.auth_session().is_none());
    assert!(state.to_setup_view_model().error.is_some());
    assert!(manager.load_profile("peixuan").unwrap().is_none());
    std::fs::remove_dir(config_path).unwrap();
    std::fs::rename(backup, config_path).unwrap();
    state.apply_input(InputEvent::from_key_label("Enter"));
    assert_eq!(state.active_screen(), ShellScreen::Home);
    assert_eq!(manager.load_config().unwrap().timezone, saved_timezone);
    assert!(manager.load_profile("peixuan").unwrap().is_some());
}

#[test]
fn linux_ignores_existing_independent_time_preferences() {
    let (_guard, manager, _) = fixture(false);
    let mut saved = manager.load_config().unwrap();
    saved.timezone = "Pacific/Honolulu".into();
    saved.time_sync.source = storage::TimeSyncSource::NetworkServer;
    saved.time_sync.server_url = Some("https://old-clock.example".into());
    manager.save_config(&saved).unwrap();
    let state = linux_state(&manager, false);
    assert!(state.app.snapshot().clock_timezone_id.is_none());
    assert_eq!(
        state.app.storage_config().time_sync.source,
        storage::TimeSyncSource::OperatingSystem
    );
    assert!(state.app.storage_config().time_sync.server_url.is_none());
    assert_eq!(manager.load_config().unwrap(), saved);
}

#[test]
fn linux_ascii_runtime_fallback_applies_to_each_existing_user_login() {
    let (_guard, manager, session) = fixture(false);
    manager
        .update_profile(&session.username, |profile| {
            profile.appearance.icon_display_mode = storage::IconDisplayMode::Image;
        })
        .unwrap();
    let mut state = linux_state(&manager, false);
    for _ in 0..2 {
        state.attach_system_session(session.clone());
        assert_eq!(state.active_screen(), ShellScreen::Home);
        assert_eq!(
            state.app.active_appearance().unwrap().icon_display_mode,
            storage::IconDisplayMode::Ascii
        );
        assert!(!state.graphical_icons_enabled());
    }
}

#[cfg(target_os = "linux")]
#[test]
fn linux_auto_login_preserves_saved_image_icons_on_a_supported_terminal() {
    let (_guard, manager, _) = fixture(false);
    let session = AuthSession::current(platform::native_platform().as_ref()).unwrap();
    manager
        .update_profile(&session.username, |profile| {
            profile.appearance.icon_display_mode = storage::IconDisplayMode::Image;
        })
        .unwrap();
    let mut state = linux_state(&manager, true);
    state.attach_system_session(session.clone());
    assert_eq!(state.active_screen(), ShellScreen::Home);
    assert!(state.graphical_icons_enabled());
    assert_eq!(
        manager
            .load_profile(&session.username)
            .unwrap()
            .unwrap()
            .appearance
            .icon_display_mode,
        storage::IconDisplayMode::Image
    );
}
