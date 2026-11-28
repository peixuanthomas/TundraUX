use super::*;
use platform::AppPaths;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(1);

struct FixtureRoot(PathBuf);

impl FixtureRoot {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "tundra-storage-reset-{label}-{}-{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).expect("fixture root");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for FixtureRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn fixture_paths(root: &Path) -> AppPaths {
    AppPaths::from_parts(
        root.join("config").join("config.toml"),
        root.join("data"),
        root.join("cache"),
        root.join("logs"),
        root.join("temp"),
    )
    .expect("absolute fixture paths")
}

#[test]
fn reset_removes_owned_content_and_recreates_defaults() {
    let fixture = FixtureRoot::new("recreate");
    let paths = fixture_paths(fixture.path());
    StorageManager::open(paths.clone()).expect("initial storage");
    fs::write(paths.data_path().join("private.txt"), "saved").expect("saved content");
    fs::create_dir_all(paths.cache_path()).expect("cache");
    fs::write(paths.cache_path().join("cache.bin"), b"cache").expect("cache content");

    let report = reset_saved_content(&paths).expect("reset");

    assert!(
        report
            .removed_paths
            .contains(&paths.config_path().to_path_buf())
    );
    assert!(
        report
            .removed_paths
            .contains(&paths.data_path().to_path_buf())
    );
    assert!(!paths.data_path().join("private.txt").exists());
    assert!(paths.config_path().exists());
    assert!(paths.data_path().exists());
    assert!(StorageManager::open(paths).is_ok());
}

#[test]
fn resetting_settings_preserves_user_content_and_other_profiles() {
    let fixture = FixtureRoot::new("settings");
    let paths = fixture_paths(fixture.path());
    let manager = StorageManager::open(paths.clone()).unwrap().manager;
    let mut config = manager.load_config().unwrap();
    config.language = "zh-CN".into();
    config.screen_timeout_seconds = 60;
    config.automation.enabled = true;
    let entry = crate::LauncherEntryRecord {
        id: "my-app".into(),
        path: "/my/app".into(),
        ..Default::default()
    };
    config.launcher.entries = vec![entry.clone()];
    config.launcher.order = vec![entry.id.clone()];
    manager.save_config(&config).unwrap();
    let custom_profile = |p: &mut UserProfile| p.appearance.animation_speed_percent = 150;
    manager.update_profile("alice", custom_profile).unwrap();
    let bob = manager.update_profile("bob", custom_profile).unwrap();
    let mut state = manager.load_state().unwrap();
    for key in [
        "home.order.1",
        "launcher.view.1",
        "home.shortcuts.1",
        "launcher.view.2",
        "saved-record",
    ] {
        state.values.insert(key.into(), "keep".into());
    }
    manager.save_state(&state).unwrap();
    let recent = crate::RecentFilesDocument {
        files: vec!["notes.md".into()],
        ..Default::default()
    };
    manager.save_recent_files(&recent).unwrap();
    let mut clock = manager.load_clock().unwrap();
    clock
        .profiles
        .entry("alice".into())
        .or_default()
        .entries
        .push(crate::ClockEntryRecord::DailyAlarm {
            id: 1,
            hour: 7,
            minute: 30,
            second: 0,
            strong: false,
            snooze_deadline_epoch_ms: None,
        });
    manager.save_clock(&clock).unwrap();
    let draft = paths.data_path().join("draft");
    let log = paths.logs_path().join("example.log");
    fs::write(&draft, "draft").unwrap();
    fs::create_dir_all(paths.logs_path()).unwrap();
    fs::write(&log, "log").unwrap();

    manager.reset_settings("alice", "1").unwrap();
    manager.reset_settings("alice", "1").unwrap();

    let mut defaults = StorageConfig::default();
    defaults.launcher.entries = vec![entry];
    assert_eq!(manager.load_config().unwrap(), defaults);
    assert_eq!(
        manager.load_profile("alice").unwrap(),
        Some(UserProfile::default())
    );
    assert_eq!(manager.load_profile("bob").unwrap(), Some(bob));
    state.values.remove("home.order.1");
    state.values.remove("launcher.view.1");
    assert_eq!(manager.load_state().unwrap(), state);
    assert_eq!(manager.load_recent_files().unwrap(), recent);
    assert_eq!(manager.load_clock().unwrap(), clock);
    assert_eq!(fs::read_to_string(draft).unwrap(), "draft");
    assert_eq!(fs::read_to_string(log).unwrap(), "log");
}

#[test]
fn settings_reset_reads_saved_preferences_before_changing_anything() {
    let fixture = FixtureRoot::new("invalid-settings");
    let manager = StorageManager::open(fixture_paths(fixture.path()))
        .unwrap()
        .manager;
    let config = StorageConfig {
        screen_timeout_seconds: 60,
        ..Default::default()
    };
    manager.save_config(&config).unwrap();
    let profile = manager
        .update_profile("alice", |p| p.appearance.animation_speed_percent = 150)
        .unwrap();
    fs::write(&manager.layout().state_path, b"invalid").unwrap();
    assert!(manager.reset_settings("alice", "1").is_err());
    assert_eq!(manager.load_config().unwrap(), config);
    assert_eq!(manager.load_profile("alice").unwrap(), Some(profile));
    assert_eq!(fs::read(&manager.layout().state_path).unwrap(), b"invalid");
}

#[test]
fn validation_rejects_overlapping_candidates_before_deleting() {
    let fixture = FixtureRoot::new("overlap");
    let root = fixture.path();
    let marker = root.join("owned").join("marker.txt");
    fs::create_dir_all(marker.parent().expect("parent")).expect("directory");
    fs::write(&marker, "keep").expect("marker");
    let paths = AppPaths::from_parts(
        root.join("owned"),
        root.join("owned").join("data"),
        root.join("cache"),
        root.join("logs"),
        root.join("temp"),
    )
    .expect("absolute paths");

    let error = reset_saved_content(&paths).expect_err("overlap must fail");

    assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    assert!(marker.exists(), "validation must happen before deletion");
}

#[cfg(unix)]
#[test]
fn reset_unlinks_owned_directory_symlinks_without_following_them() {
    let fixture = FixtureRoot::new("symlink");
    let paths = fixture_paths(fixture.path());
    StorageManager::open(paths.clone()).expect("initial storage");
    let external = fixture.path().join("outside-owned-paths");
    fs::create_dir_all(&external).expect("external directory");
    let marker = external.join("keep.txt");
    fs::write(&marker, "keep").expect("external marker");
    if paths.cache_path().try_exists().expect("cache exists check") {
        fs::remove_dir_all(paths.cache_path()).expect("remove cache directory");
    }
    std::os::unix::fs::symlink(&external, paths.cache_path()).expect("cache symlink");

    reset_saved_content(&paths).expect("reset");

    assert!(marker.exists(), "reset must not follow directory symlinks");
    assert!(!paths.cache_path().is_symlink());
}
