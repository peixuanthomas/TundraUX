use storage::{DEFAULT_SCREEN_TIMEOUT_SECONDS, StorageConfig};

#[test]
fn missing_timeout_uses_five_minutes_and_saved_timeouts_round_trip() {
    let config: StorageConfig = toml::from_str("schema_version = 3").unwrap();
    assert_eq!(
        config.screen_timeout_seconds,
        DEFAULT_SCREEN_TIMEOUT_SECONDS
    );
    for seconds in [0, 30, 60, 120, 300, 600, 1800] {
        let config = StorageConfig {
            screen_timeout_seconds: seconds,
            ..config.clone()
        };
        let loaded: StorageConfig = toml::from_str(&toml::to_string(&config).unwrap()).unwrap();
        assert_eq!(loaded.screen_timeout_seconds, seconds);
    }
}
