use super::*;
use ratatui::{Terminal, backend::TestBackend};

struct TempDirectory(PathBuf);

impl Drop for TempDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn storage() -> (TempDirectory, AppPaths, StorageManager) {
    let root = TempDirectory(std::env::temp_dir().join(format!(
        "tundra-cell-icon-test-{}-{}",
        std::process::id(),
        PageInstanceId::new().get()
    )));
    let paths = AppPaths::from_parts(
        root.0.join("config.toml"),
        root.0.join("data"),
        root.0.join("cache"),
        root.0.join("logs"),
        root.0.join("temp"),
    )
    .unwrap();
    let storage = StorageManager::open(paths.clone()).unwrap().manager;
    (root, paths, storage)
}

fn session(storage: StorageManager) -> ShellSession {
    let ascii_assets = ui::RuntimeAsciiAssets::load_default().unwrap();
    let mut startup = ShellStartupState::clean(
        PlatformKind::Linux,
        PlatformCapabilities::native_supported(),
    );
    startup.storage_manager = Some(storage);
    ShellSession::new_with_startup_and_assets(
        ShellLaunchConfig::default(),
        (108, 20),
        startup,
        ascii_assets,
    )
}

fn check(install: CellIconFontInstall, preference: Option<Preference>, manual: bool) -> Check {
    Check {
        preference_path: None,
        preference,
        install: Ok(install),
        manual,
    }
}

#[test]
fn only_ready_shells_check_fonts_after_personalization() {
    let (_root, paths, storage) = storage();
    let mut state = session(storage.clone());
    let auth = AuthSession {
        source: identity::IdentitySource::LinuxCurrentProcess,
        session_id: "font-test".into(),
        user_id: "test-user".into(),
        username: "test-user".into(),
        role: UserRole::User,
        started_at_epoch_ms: 1,
    };
    state.attach_system_session(auth.clone());
    assert_eq!(state.active_screen(), ShellScreen::FirstRunSetup);
    assert!(!ready_for_check(&state));
    state.finish_first_run_setup();
    assert_eq!(state.active_screen(), ShellScreen::Home);
    assert!(ready_for_check(&state));

    let mut reopened = session(storage);
    reopened.attach_system_session(auth.clone());
    assert!(ready_for_check(&reopened));
    reopened.open_settings();
    let settings = reopened.to_settings_view_model().unwrap();
    let mut items = settings.cards.iter().flat_map(|card| &card.items);
    let recheck_index = items
        .position(|item| item.field == ui::SettingsField::RecheckCellIcons)
        .unwrap();
    reopened.select_settings_field_at(recheck_index);
    reopened.apply_input_with_platform(
        InputEvent::from_key_label("Enter"),
        &platform::mock::UnsupportedPlatform,
    );
    assert!(reopened.cell_icon_recheck_requested);
    reopened.window_owner =
        window_launch::WindowOwner::App(window_launch::WindowApplication::Settings);
    assert!(!ready_for_check(&reopened));

    storage::reset_saved_content(&paths).unwrap();
    let mut reset = session(StorageManager::open(paths).unwrap().manager);
    reset.attach_system_session(auth);
    assert_eq!(reset.active_screen(), ShellScreen::FirstRunSetup);
    assert!(!ready_for_check(&reset));
}

#[test]
fn availability_alone_never_confirms_a_font() {
    use CellIconFontInstall::*;
    let font = || {
        Some(Preference::from_choice(
            CellIconChoice::Font,
            FontAvailability::Present,
        ))
    };
    let ascii = || {
        Some(Preference::from_choice(
            CellIconChoice::Ascii,
            FontAvailability::Present,
        ))
    };
    assert_eq!(check(Available, None, false).existing_mode(), None);
    assert_eq!(check(Installed, None, false).existing_mode(), None);
    assert_eq!(check(Conflict, None, false).existing_mode(), None);
    assert_eq!(
        check(Available, font(), false).existing_mode(),
        Some(CellIconMode::Font),
    );
    assert_eq!(check(Installed, font(), false).existing_mode(), None);
    assert_eq!(
        check(Available, ascii(), false).existing_mode(),
        Some(CellIconMode::Ascii),
    );
    assert_eq!(check(Available, ascii(), true).existing_mode(), None);
    assert_eq!(check(Installed, ascii(), false).existing_mode(), None);
    assert_eq!(check(Conflict, ascii(), false).existing_mode(), None);
    for mode in [CellIconMode::Font, CellIconMode::Ascii] {
        let previous_atlas = Preference::Selected {
            mode,
            revision: "67e0a0b1c1c8da6136f850cc5fc27f9c1c974c66d0b643dcc56f4637cfbb0971".into(),
            availability: FontAvailability::Present,
        };
        assert_eq!(
            check(Available, Some(previous_atlas), false).existing_mode(),
            None,
        );
    }
    assert_eq!(
        check(Installed, Some(Preference::Suppressed), false).existing_mode(),
        Some(CellIconMode::Ascii),
    );
    assert_eq!(
        check(Available, Some(Preference::Suppressed), true).existing_mode(),
        None
    );
}

#[test]
fn profile_choices_are_independent_files_and_ascii_can_replace_confirmation() {
    let (_root, paths, _) = storage();
    let first = preference_path(&paths, "windows-terminal:1");
    let second = preference_path(&paths, "windows-terminal:2");
    let availability = FontAvailability::Present;
    save_preference(&first, CellIconChoice::Font, availability).unwrap();
    save_preference(&second, CellIconChoice::Suppress, availability).unwrap();
    assert_eq!(
        load_preference(&first),
        Some(Preference::from_choice(CellIconChoice::Font, availability))
    );
    assert_eq!(load_preference(&second), Some(Preference::Suppressed));
    save_preference(&first, CellIconChoice::Ascii, availability).unwrap();
    assert_eq!(
        load_preference(&first),
        Some(Preference::from_choice(CellIconChoice::Ascii, availability))
    );
    assert_eq!(load_preference(&second), Some(Preference::Suppressed));
}

#[test]
fn terminal_id_distinguishes_profiles_and_discards_remote_and_multiplexed_environment() {
    let profile = |variables: &[(&str, &str)]| {
        terminal_profile(
            |key| {
                variables
                    .iter()
                    .find(|(name, _)| *name == key)
                    .map(|(_, value)| (*value).into())
            },
            || Some("test-gnome-profile".into()),
        )
    };
    assert_eq!(
        profile(&[("WT_SESSION", "tab-1"), ("WT_PROFILE_ID", "{TEST-ID}")]),
        profile(&[("WT_SESSION", "tab-2"), ("WT_PROFILE_ID", "test-id")]),
    );
    assert_ne!(
        profile(&[("WT_SESSION", "tab"), ("WT_PROFILE_ID", "profile-a")]),
        profile(&[("WT_SESSION", "tab"), ("WT_PROFILE_ID", "profile-b")]),
    );
    assert_eq!(profile(&[("WT_SESSION", "tab")]), None);
    assert_eq!(
        profile(&[("TERM", "xterm-kitty"), ("KITTY_WINDOW_ID", "123")]).as_deref(),
        Some("kitty:")
    );
    assert_eq!(profile(&[("TERM", "xterm-kitty")]), None);
    assert_eq!(
        profile(&[
            ("TERM_PROGRAM", "Apple_Terminal"),
            ("TERM_SESSION_ID", "session-1")
        ]),
        None
    );
    assert_eq!(
        profile(&[("TERM_PROGRAM", "iTerm.app"), ("ITERM_PROFILE", "Work")]).as_deref(),
        Some("iterm2:Work")
    );
    assert_ne!(
        profile(&[("TERM_PROGRAM", "iTerm.app"), ("ITERM_PROFILE", "Work")]),
        profile(&[("TERM_PROGRAM", "iTerm.app"), ("ITERM_PROFILE", "Personal")]),
    );
    assert_eq!(
        profile(&[("GNOME_TERMINAL_SCREEN", "test-tab")]).as_deref(),
        Some("gnome-terminal:test-gnome-profile")
    );
    assert_ne!(
        profile(&[
            ("TERM_PROGRAM", "WezTerm"),
            ("WEZTERM_CONFIG_FILE", "/etc/a.lua")
        ]),
        profile(&[
            ("TERM_PROGRAM", "WezTerm"),
            ("WEZTERM_CONFIG_FILE", "/etc/b.lua")
        ]),
    );
    assert_eq!(
        profile(&[
            ("TERM_PROGRAM", "WezTerm"),
            ("GNOME_TERMINAL_SCREEN", "old-tab")
        ]),
        profile(&[("TERM_PROGRAM", "WezTerm")]),
    );
    assert_eq!(
        profile(&[("VTE_VERSION", "7800"), ("TERM", "xterm-256color")]),
        None
    );
    assert_eq!(
        profile(&[("SSH_TTY", "/dev/pts/0"), ("TERM", "xterm-kitty")]),
        None
    );
    assert_eq!(
        profile(&[
            ("TMUX", "socket"),
            ("WT_SESSION", "tab"),
            ("WT_PROFILE_ID", "profile")
        ]),
        None
    );
    for hint in [
        ("XTERM_VERSION", "XTerm(399)"),
        ("TERM", "linux"),
        ("TERM", "screen"),
        ("TERM", "screen.xterm-256color"),
        ("TERM", "tmux-256color"),
    ] {
        assert_eq!(
            profile(&[
                ("WT_SESSION", "inherited-tab"),
                ("WT_PROFILE_ID", "profile"),
                hint
            ]),
            None,
            "{hint:?} overrides the inherited Windows Terminal profile"
        );
        assert_eq!(
            profile(&[
                ("TERM_PROGRAM", "WezTerm"),
                ("WEZTERM_CONFIG_FILE", "/etc/a.lua"),
                hint
            ]),
            None,
            "{hint:?} overrides the inherited WezTerm profile"
        );
    }
}

#[test]
fn confirmation_draws_the_real_glyph_and_an_aligned_ascii_sample_in_both_languages() {
    let (_root, paths, storage) = storage();
    let mut state = session(storage);
    let glyph = ui::CellIconMode::Font.icon("fileops", "delete");
    assert_ne!(glyph, ui::CellIconMode::Ascii.icon("fileops", "delete"));
    let mut check = check(CellIconFontInstall::Available, None, false);
    check.preference_path = Some(preference_path(&paths, "windows-terminal:1"));
    for language in ["en-US", "zh-CN"] {
        let (catalog, snapshot) = state.prepare_language(language).unwrap();
        state.publish_language(catalog, snapshot);
        let _language = i18n::enter_snapshot(state.language.clone());
        show_check_dialog(&mut state, &check);
        let model = state.notification_active_modal_view_model().unwrap();
        let mut terminal = Terminal::new(TestBackend::new(108, 20)).unwrap();
        let theme = ui::TundraTheme::default_dark();
        let drawn = terminal
            .draw(|frame| {
                ui::render_notification_overlay(frame, frame.area(), &model, &theme);
            })
            .unwrap();
        let row_position = |middle: &str| {
            (0..20)
                .find_map(|y| {
                    (0..106)
                        .find(|&x| {
                            drawn.buffer[(x, y)].symbol() == "A"
                                && drawn.buffer[(x + 1, y)].symbol() == middle
                                && drawn.buffer[(x + 2, y)].symbol() == "B"
                        })
                        .map(|x| (x, y))
                })
                .unwrap()
        };
        let (font_x, font_y) = row_position(glyph);
        let (ascii_x, ascii_y) = row_position(".");
        assert_eq!(ascii_x, font_x);
        assert_eq!(ascii_y, font_y + 1);
        assert_eq!(
            model
                .actions
                .iter()
                .map(|action| action.id.as_str())
                .collect::<Vec<_>>(),
            ["use-ascii", "suppress", "looks-right", "close"]
        );
        state.notification_dismiss_active_modal_without_response();
    }

    check.preference_path = None;
    show_check_dialog(&mut state, &check);
    assert!(
        !state
            .notification_active_modal_view_model()
            .unwrap()
            .actions
            .iter()
            .any(|action| action.id == "suppress")
    );
}
