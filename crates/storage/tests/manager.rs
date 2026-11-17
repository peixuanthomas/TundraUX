use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use platform::{AppPaths, cleanup_temp_path};
use storage::{
    AppearanceConfig, BorderColor, BorderShape, ClockDocument, ClockEntryRecord, ClockProfile,
    EditorConfig, ExplorerConfig, ExplorerDateZone, ExplorerSizeFormat, ExplorerSortDirection,
    ExplorerSortField, IconDisplayMode, LauncherConfig, LauncherEntryRecord,
    LauncherExecutableKind, MotionPreference, RecentFilesDocument, SCHEMA_VERSION,
    SessionsDocument, StateDocument, StorageConfig, StorageError, StorageLayout, StorageManager,
    TimeSyncConfig, TimeSyncSource, TrashDocument, TrashRecord,
};

#[test]
fn first_open_creates_storage_directories_and_default_files() {
    let base = unique_temp_root("first-open");
    let paths = app_paths(&base);
    let layout = StorageLayout::from_app_paths(&paths);

    let opened = StorageManager::open(paths).expect("storage should open from empty paths");

    assert!(layout.config_path.parent().expect("config parent").is_dir());
    assert!(layout.data_path.is_dir());
    assert!(layout.cache_path.is_dir());
    assert!(layout.logs_path.is_dir());
    assert!(layout.temp_path.is_dir());
    assert!(layout.config_path.is_file());
    assert!(layout.profiles_path.is_file());
    assert!(layout.state_path.is_file());
    assert!(layout.recent_files_path.is_file());
    assert!(layout.sessions_path.is_file());
    assert!(layout.clock_path.is_file());
    assert!(layout.trash_path.is_dir());
    assert!(layout.trash_manifest_path.is_file());
    assert_eq!(
        opened
            .manager
            .load_trash()
            .expect("default trash manifest")
            .records,
        Vec::<TrashRecord>::new()
    );
    assert_eq!(
        sorted_paths(opened.report.created_files),
        sorted_paths(vec![
            layout.config_path,
            layout.profiles_path,
            layout.state_path,
            layout.recent_files_path,
            layout.sessions_path,
            layout.clock_path,
            layout.trash_manifest_path,
        ])
    );
    assert!(opened.report.migrated_files.is_empty());
    assert!(opened.report.recovered_files.is_empty());
    assert!(opened.report.warnings.is_empty());

    cleanup(&base);
}

#[cfg(unix)]
#[test]
fn reopening_storage_tightens_only_owned_paths() {
    use std::os::unix::fs::PermissionsExt;

    let base = unique_temp_root("private-permissions");
    let paths = app_paths(&base);
    let layout = StorageLayout::from_app_paths(&paths);
    StorageManager::open(paths.clone()).expect("initial storage open");

    fs::set_permissions(&layout.data_path, fs::Permissions::from_mode(0o755)).unwrap();
    fs::set_permissions(&layout.profiles_path, fs::Permissions::from_mode(0o644)).unwrap();
    let editor_document = base.join("document-owned-by-user.txt");
    fs::write(&editor_document, "leave this permission alone").unwrap();
    fs::set_permissions(&editor_document, fs::Permissions::from_mode(0o644)).unwrap();

    StorageManager::open(paths).expect("storage re-open tightens its own paths");

    assert_eq!(
        fs::metadata(&layout.data_path)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(&layout.profiles_path)
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(&editor_document).unwrap().permissions().mode() & 0o777,
        0o644
    );
    cleanup(&base);
}

#[test]
fn editor_suffixes_preserve_explicit_lists() {
    for (case, extensions) in [
        ("custom", vec!["txt".to_string(), "rs".to_string()]),
        ("empty", Vec::new()),
        ("default", EditorConfig::default().explorer_open_extensions),
    ] {
        let base = unique_temp_root(case);
        let paths = app_paths(&base);
        let layout = StorageLayout::from_app_paths(&paths);
        fs::create_dir_all(layout.config_path.parent().unwrap()).unwrap();
        let mut config = StorageConfig::default();
        config.editor.vim_mode_enabled = true;
        config.editor.cursor_acceleration_delay_ms = 1_234;
        config.editor.explorer_open_extensions = extensions;
        fs::write(&layout.config_path, toml::to_string(&config).unwrap()).unwrap();

        let opened = StorageManager::open(paths).unwrap();
        assert_eq!(opened.manager.load_config().unwrap(), config, "{case}");
        let persisted: StorageConfig =
            toml::from_str(&fs::read_to_string(&layout.config_path).unwrap()).unwrap();
        assert_eq!(persisted, config, "{case}");
        assert!(opened.report.migrated_files.is_empty(), "{case}");
        cleanup(&base);
    }
}

#[test]
fn toml_and_json_documents_round_trip() {
    let base = unique_temp_root("round-trip");
    let opened = StorageManager::open(app_paths(&base)).expect("storage should open");
    let manager = opened.manager;

    let mut shortcuts = BTreeMap::new();
    shortcuts.insert("open_launcher".to_string(), "Ctrl+Space".to_string());
    let config = StorageConfig {
        session_revision: None,
        session_authorization_revision: None,
        schema_version: SCHEMA_VERSION,
        screen_timeout_seconds: 120,
        automation: storage::AutomationConfig { enabled: true },
        linux_update_mode: storage::LinuxUpdateMode::Beta,
        auto_admin: storage::AutoAdminPolicy::Manual,
        theme: "light".to_string(),
        language: "zh-Hans".to_string(),
        timezone: "Asia/Shanghai".to_string(),
        time_sync: TimeSyncConfig {
            source: TimeSyncSource::OperatingSystem,
            server_url: Some("https://time.example.test/".to_string()),
        },
        weather_location: Some("Pudong, Shanghai, China".to_string()),
        shortcuts,
        appearance: AppearanceConfig {
            border_shape: BorderShape::Square,
            border_color: BorderColor::Rgb(0x38, 0xBD, 0xF8),
            accent_color: BorderColor::LightMagenta,
            icon_display_mode: IconDisplayMode::Ascii,
            motion_preference: MotionPreference::Reduced,
            animation_speed_percent: 125,
            animation_timings: storage::AnimationTimingsConfig::default(),
        },
        explorer: ExplorerConfig {
            show_hidden: true,
            show_system: true,
            show_extensions: false,
            folders_first: false,
            case_sensitive_sort: true,
            size_format: ExplorerSizeFormat::Bytes,
            date_zone: ExplorerDateZone::Utc,
            confirm_delete: false,
            confirm_name_conflicts: false,
            show_sidebar: false,
            sort_field: ExplorerSortField::Modified,
            sort_direction: ExplorerSortDirection::Descending,
        },
        editor: EditorConfig {
            vim_mode_enabled: true,
            explorer_open_extensions: vec!["md".to_string(), "rs".to_string()],
            cursor_acceleration_enabled: false,
            cursor_acceleration_delay_ms: 1_500,
            cursor_acceleration_ramp_ms: 2_500,
            cursor_horizontal_max_step: 10,
            cursor_vertical_max_step: 4,
        },
        launcher: LauncherConfig {
            entries: vec![LauncherEntryRecord {
                id: "notepad".to_string(),
                path: "C:/Windows/notepad.exe".to_string(),
                executable_kind: Some(LauncherExecutableKind::NativeBinary),
                added_by_user_id: "admin".to_string(),
                added_at_epoch_ms: 99,
                terminal: None,
                custom: None,
                name: None,
                icon: None,
            }],
            ..LauncherConfig::default()
        },
        system_status: storage::SystemStatusConfig::default(),
        runtime_logs: storage::RuntimeLogsConfig::default(),
    };
    manager
        .save_config(&config)
        .expect("config should save atomically");
    let mut expected_config = config.clone();
    expected_config.language = "zh-CN".to_string();
    assert_eq!(
        manager.load_config().expect("config should reload"),
        expected_config
    );
    let config_contents =
        fs::read_to_string(&manager.layout().config_path).expect("config should be readable");
    assert!(config_contents.contains("language = \"zh-CN\""));
    assert!(config_contents.contains("timezone = \"Asia/Shanghai\""));
    assert!(config_contents.contains("weather_location = \"Pudong, Shanghai, China\""));
    assert!(config_contents.contains("[appearance]"));
    assert!(config_contents.contains("border_shape = \"square\""));
    assert!(config_contents.contains("border_color = \"#38BDF8\""));
    assert!(config_contents.contains("accent_color = \"light-magenta\""));
    assert!(config_contents.contains("icon_display_mode = \"ascii\""));
    assert!(config_contents.contains("motion_preference = \"reduced\""));
    assert!(config_contents.contains("theme = \"light\""));
    assert!(config_contents.contains("size_format = \"bytes\""));
    assert!(config_contents.contains("date_zone = \"utc\""));
    assert!(config_contents.contains("sort_field = \"modified\""));
    assert!(config_contents.contains("sort_direction = \"descending\""));

    let appearance = AppearanceConfig {
        border_shape: BorderShape::Square,
        ..AppearanceConfig::default()
    };
    let saved = manager
        .update_profile("系统用户", |profile| {
            profile.appearance = appearance.clone()
        })
        .unwrap();
    assert_eq!(saved.appearance, appearance);
    assert_eq!(manager.load_profile("系统用户").unwrap(), Some(saved));
    assert_eq!(manager.load_profile("other-user").unwrap(), None);

    let mut values = BTreeMap::new();
    values.insert("active_view".to_string(), "home".to_string());
    let state = StateDocument {
        schema_version: SCHEMA_VERSION,
        values,
    };
    manager.save_state(&state).expect("state should save");
    assert_eq!(manager.load_state().expect("state should reload"), state);

    let recent = RecentFilesDocument {
        schema_version: SCHEMA_VERSION,
        files: vec!["C:/Projects/readme.md".to_string()],
    };
    manager
        .save_recent_files(&recent)
        .expect("recent files should save");
    assert_eq!(
        manager
            .load_recent_files()
            .expect("recent files should reload"),
        recent
    );

    let sessions = SessionsDocument {
        schema_version: SCHEMA_VERSION,
        sessions: vec!["default".to_string()],
    };
    manager
        .save_sessions(&sessions)
        .expect("sessions should save");
    assert_eq!(
        manager.load_sessions().expect("sessions should reload"),
        sessions
    );

    let mut profiles = BTreeMap::new();
    profiles.insert(
        "user-1".to_string(),
        ClockProfile {
            next_id: 3,
            entries: vec![
                ClockEntryRecord::DailyAlarm {
                    id: 1,
                    hour: 7,
                    minute: 30,
                    second: 15,
                    strong: true,
                    snooze_deadline_epoch_ms: Some(1_725_000_000_000),
                },
                ClockEntryRecord::Countdown {
                    id: 2,
                    deadline_epoch_ms: 1_725_000_300_000,
                    strong: false,
                },
            ],
        },
    );
    let clock = ClockDocument {
        schema_version: SCHEMA_VERSION,
        profiles,
    };
    manager.save_clock(&clock).expect("clock should save");
    assert_eq!(manager.load_clock().expect("clock should reload"), clock);
    let clock_contents =
        fs::read_to_string(&manager.layout().clock_path).expect("clock should be readable");
    assert!(clock_contents.contains("\"kind\": \"daily_alarm\""));
    assert!(clock_contents.contains("\"kind\": \"countdown\""));

    let trash = TrashDocument {
        schema_version: SCHEMA_VERSION,
        records: vec![TrashRecord {
            original_path: PathBuf::from("C:/Projects/readme.md"),
            trash_path: PathBuf::from("C:/TundraUX/trash/readme.md"),
            actor: "local-user".to_string(),
            timestamp_epoch_ms: 42,
        }],
    };
    manager.save_trash(&trash).expect("trash should save");
    assert_eq!(manager.load_trash().expect("trash should reload"), trash);

    cleanup(&base);
}

#[test]
fn invalid_schema_is_rejected_without_overwriting_data() {
    let base = unique_temp_root("invalid-schema");
    let paths = app_paths(&base);
    let path = paths.config_path().to_path_buf();
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let invalid = "schema_version = 0\ntheme = \"dark\"\n";
    fs::write(&path, invalid).unwrap();
    assert!(matches!(
        StorageManager::open(paths),
        Err(StorageError::InvalidSchemaVersion { found: 0, .. })
    ));
    assert_eq!(fs::read_to_string(&path).unwrap(), invalid);
    cleanup(&base);
}

#[test]
fn missing_language_or_timezone_uses_defaults() {
    let base = unique_temp_root("missing-config-defaults");
    let paths = app_paths(&base);
    let layout = StorageLayout::from_app_paths(&paths);
    fs::create_dir_all(layout.config_path.parent().expect("config parent"))
        .expect("config parent should be writable");
    fs::write(
        &layout.config_path,
        "schema_version = 3\ntheme = \"light\"\n\n[shortcuts]\n\n[explorer]\nshow_hidden = true\n\n[launcher]\n\n",
    )
    .expect("config fixture");

    let opened = StorageManager::open(paths).expect("config should load");
    let config = opened.manager.load_config().expect("config should load");

    assert_eq!(config.theme, "light");
    assert_eq!(config.language, "en-US");
    assert_eq!(config.timezone, "UTC");
    assert_eq!(config.time_sync, TimeSyncConfig::default());
    assert_eq!(config.weather_location, None);
    assert_eq!(config.appearance.border_shape, BorderShape::Rounded);
    assert_eq!(
        config.appearance.border_color,
        BorderColor::Rgb(0x29, 0x43, 0x4e)
    );
    assert_eq!(
        config.appearance.accent_color,
        BorderColor::Rgb(0x63, 0xD3, 0xE5)
    );
    assert_eq!(config.appearance.motion_preference, MotionPreference::Full);
    assert_eq!(config.schema_version, SCHEMA_VERSION);
    assert_eq!(config.editor, EditorConfig::default());
    assert_eq!(
        config.explorer,
        ExplorerConfig {
            show_hidden: true,
            ..ExplorerConfig::default()
        }
    );
    assert!(opened.report.recovered_files.is_empty());
    assert!(opened.report.warnings.is_empty());

    cleanup(&base);
}

#[test]
fn existing_non_english_config_is_canonicalized_without_losing_language_on_open() {
    let base = unique_temp_root("non-english-config");
    let paths = app_paths(&base);
    let layout = StorageLayout::from_app_paths(&paths);
    fs::create_dir_all(layout.config_path.parent().expect("config parent"))
        .expect("config parent should be writable");
    fs::write(
        &layout.config_path,
        "schema_version = 3\ntheme = \"dark\"\nlanguage = \"zh-Hans\"\ntimezone = \"UTC\"\n",
    )
    .expect("language alias fixture");

    let opened = StorageManager::open(paths).expect("config normalization should succeed");

    assert_eq!(
        opened.manager.load_config().expect("config").language,
        "zh-CN"
    );
    assert!(opened.report.migrated_files.contains(&layout.config_path));
    let contents = fs::read_to_string(&layout.config_path).expect("migrated config is readable");
    assert!(contents.contains("language = \"zh-CN\""));
    assert!(!contents.contains("zh-Hans"));

    cleanup(&base);
}

#[test]
fn clock_fields_missing_from_hand_written_files_use_defaults() {
    let base = unique_temp_root("clock-field-defaults");
    let paths = app_paths(&base);
    let layout = StorageLayout::from_app_paths(&paths);
    fs::create_dir_all(&layout.data_path).expect("data path should be writable");
    fs::write(
        &layout.clock_path,
        "{\n  \"schema_version\": 3,\n  \"profiles\": {\n    \"user-1\": {\n      \"entries\": [\n        { \"kind\": \"daily_alarm\" },\n        { \"kind\": \"countdown\" }\n      ]\n    }\n  }\n}\n",
    )
    .expect("hand-written clock fixture");

    let opened = StorageManager::open(paths).expect("clock defaults should load");
    let clock = opened.manager.load_clock().expect("clock should load");
    let profile = clock.profiles.get("user-1").expect("profile should exist");

    assert_eq!(profile.next_id, 1);
    assert_eq!(
        profile.entries,
        vec![
            ClockEntryRecord::DailyAlarm {
                id: 0,
                hour: 0,
                minute: 0,
                second: 0,
                strong: false,
                snooze_deadline_epoch_ms: None,
            },
            ClockEntryRecord::Countdown {
                id: 0,
                deadline_epoch_ms: 0,
                strong: false,
            },
        ]
    );
    assert!(opened.report.recovered_files.is_empty());

    cleanup(&base);
}

#[test]
fn corrupt_toml_and_json_are_backed_up_and_replaced_with_defaults() {
    let base = unique_temp_root("corrupt-recovery");
    let paths = app_paths(&base);
    let layout = StorageLayout::from_app_paths(&paths);
    fs::create_dir_all(layout.config_path.parent().expect("config parent"))
        .expect("config parent should be writable");
    fs::create_dir_all(&layout.data_path).expect("data path should be writable");
    fs::create_dir_all(&layout.trash_path).expect("trash path should be writable");
    fs::write(&layout.config_path, b"schema_version =\n").expect("corrupt TOML fixture");
    fs::write(&layout.profiles_path, b"{").expect("corrupt JSON fixture");
    fs::write(&layout.clock_path, b"{").expect("corrupt clock fixture");
    fs::write(&layout.trash_manifest_path, b"{").expect("corrupt trash fixture");

    let opened = StorageManager::open(paths).expect("storage should recover corrupt documents");

    assert_eq!(
        opened.manager.load_config().expect("default config").theme,
        "dark"
    );
    assert!(
        opened
            .manager
            .load_profile("operator")
            .expect("default profiles")
            .is_none()
    );
    assert!(
        opened
            .manager
            .load_clock()
            .expect("default clock")
            .profiles
            .is_empty()
    );
    assert!(
        opened
            .manager
            .load_trash()
            .expect("default trash")
            .records
            .is_empty()
    );
    assert_eq!(opened.report.recovered_files.len(), 4);
    assert_eq!(opened.report.warnings.len(), 4);
    for recovered in &opened.report.recovered_files {
        assert!(recovered.recovered_path.is_file());
        assert!(
            recovered
                .recovered_path
                .file_name()
                .expect("backup file name")
                .to_string_lossy()
                .contains(".corrupt.")
        );
    }

    cleanup(&base);
}

#[test]
fn future_trash_schema_errors_without_modifying_file() {
    let base = unique_temp_root("future-trash");
    let paths = app_paths(&base);
    let layout = StorageLayout::from_app_paths(&paths);
    fs::create_dir_all(&layout.trash_path).expect("trash path should be writable");
    let original = format!(
        "{{\n  \"schema_version\": {},\n  \"records\": []\n}}\n",
        SCHEMA_VERSION + 1
    );
    fs::write(&layout.trash_manifest_path, &original).expect("future trash fixture");

    let error = StorageManager::open(paths).expect_err("future trash should fail");

    assert!(matches!(
        error,
        StorageError::UnsupportedSchema {
            document: "trash",
            found,
            supported: SCHEMA_VERSION,
            ..
        } if found == SCHEMA_VERSION + 1
    ));
    assert_eq!(
        fs::read_to_string(&layout.trash_manifest_path)
            .expect("future trash should remain readable"),
        original
    );
    assert_no_corrupt_backups(&layout.trash_path);

    cleanup(&base);
}

#[test]
fn future_clock_schema_errors_without_modifying_file() {
    let base = unique_temp_root("future-clock");
    let paths = app_paths(&base);
    let layout = StorageLayout::from_app_paths(&paths);
    fs::create_dir_all(&layout.data_path).expect("data path should be writable");
    let original = format!(
        "{{\n  \"schema_version\": {},\n  \"profiles\": {{}}\n}}\n",
        SCHEMA_VERSION + 1
    );
    fs::write(&layout.clock_path, &original).expect("future clock fixture");

    let error = StorageManager::open(paths).expect_err("future clock should fail");

    assert!(matches!(
        error,
        StorageError::UnsupportedSchema {
            document: "clock",
            found,
            supported: SCHEMA_VERSION,
            ..
        } if found == SCHEMA_VERSION + 1
    ));
    assert_eq!(
        fs::read_to_string(&layout.clock_path).expect("future clock should remain readable"),
        original
    );
    assert_no_corrupt_backups(&layout.data_path);

    cleanup(&base);
}

#[test]
fn future_toml_schema_errors_without_modifying_file() {
    let base = unique_temp_root("future-config");
    let paths = app_paths(&base);
    let layout = StorageLayout::from_app_paths(&paths);
    fs::create_dir_all(layout.config_path.parent().expect("config parent"))
        .expect("config parent should be writable");
    let original = format!(
        "schema_version = {}\ntheme = \"future\"\n",
        SCHEMA_VERSION + 1
    );
    fs::write(&layout.config_path, &original).expect("future TOML fixture");

    let error = StorageManager::open(paths).expect_err("future config should fail");

    assert!(matches!(
        error,
        StorageError::UnsupportedSchema {
            document: "config",
            found,
            supported: SCHEMA_VERSION,
            ..
        } if found == SCHEMA_VERSION + 1
    ));
    assert_eq!(
        fs::read_to_string(&layout.config_path).expect("future config should remain readable"),
        original
    );
    assert_no_corrupt_backups(layout.config_path.parent().expect("config parent"));

    cleanup(&base);
}

#[test]
fn future_json_schema_errors_without_modifying_file() {
    let base = unique_temp_root("future-json");
    let paths = app_paths(&base);
    let layout = StorageLayout::from_app_paths(&paths);
    fs::create_dir_all(layout.config_path.parent().expect("config parent"))
        .expect("config parent should be writable");
    fs::create_dir_all(&layout.data_path).expect("data path should be writable");
    fs::write(
        &layout.config_path,
        "schema_version = 3\ntheme = \"dark\"\n\n[shortcuts]\n\n[explorer]\nshow_hidden = false\n\n[launcher]\n",
    )
    .expect("current TOML fixture");
    let original = "{\n  \"schema_version\": 5,\n  \"profiles\": {}\n}\n";
    fs::write(&layout.profiles_path, original).expect("future JSON fixture");

    let error = StorageManager::open(paths).expect_err("future JSON should fail");

    assert!(matches!(
        error,
        StorageError::UnsupportedSchema {
            document: "profiles",
            found: 5,
            supported: SCHEMA_VERSION,
            ..
        }
    ));
    assert_eq!(
        fs::read_to_string(&layout.profiles_path).expect("future JSON should remain readable"),
        original
    );
    assert_no_corrupt_backups(&layout.data_path);

    cleanup(&base);
}

fn app_paths(base: &Path) -> AppPaths {
    AppPaths::from_parts(
        base.join("config").join("config.toml"),
        base.join("state"),
        base.join("cache"),
        base.join("logs"),
        base.join("temp"),
    )
    .expect("fixture paths should be absolute")
}

fn unique_temp_root(case: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time should be after unix epoch")
        .as_nanos();

    std::env::temp_dir().join(format!(
        "tundra-storage-test-{}-{}-{case}",
        std::process::id(),
        nanos
    ))
}

fn sorted_paths(mut paths: Vec<PathBuf>) -> Vec<PathBuf> {
    paths.sort();
    paths
}

fn assert_no_corrupt_backups(directory: &Path) {
    if !directory.exists() {
        return;
    }

    for entry in fs::read_dir(directory).expect("directory should be readable") {
        let entry = entry.expect("directory entry should be readable");
        assert!(
            !entry.file_name().to_string_lossy().contains(".corrupt."),
            "unexpected corrupt backup at {}",
            entry.path().display()
        );
    }
}

fn cleanup(path: &Path) {
    cleanup_temp_path(path).expect("test directory should be removable");
}
