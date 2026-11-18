use app::BUILT_IN_APPLICATIONS;
use platform::PlatformKind;

#[test]
fn management_logs_and_scripts_keep_platform_visibility_and_distinct_entry_points() {
    for id in [
        "builtin.services",
        "builtin.processes",
        "builtin.packages",
        "builtin.network",
        "builtin.disks",
        "builtin.firewall",
        "builtin.ssh",
        "builtin.backups",
        "builtin.scheduled-tasks",
        "builtin.logs",
        "builtin.script-editor",
    ] {
        let application = BUILT_IN_APPLICATIONS
            .iter()
            .find(|item| item.id == id)
            .expect("built-in application");
        assert!(!application.admin_only);
        assert!(!application.description.is_empty());
        assert_eq!(
            application.available_on_platform(PlatformKind::Windows),
            matches!(id, "builtin.logs" | "builtin.packages")
        );
        assert_eq!(
            application.available_on_platform(PlatformKind::Macos),
            matches!(id, "builtin.logs" | "builtin.packages")
        );
        assert!(application.available_on_platform(PlatformKind::Linux));
    }
}

#[test]
fn user_management_is_a_linux_home_application() {
    let users = BUILT_IN_APPLICATIONS
        .iter()
        .find(|item| item.id == "builtin.users")
        .expect("user management entry");
    assert!(!users.admin_only);
    for platform in [
        PlatformKind::Linux,
        PlatformKind::Windows,
        PlatformKind::Macos,
    ] {
        assert_eq!(
            users.available_on_platform(platform),
            platform == PlatformKind::Linux
        );
    }
}
