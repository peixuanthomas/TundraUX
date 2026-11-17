use identity::{
    AuthSession, DebugPolicy, IdentitySource, PermissionAction, PermissionService, UserRole,
};

fn session(source: IdentitySource, role: UserRole) -> AuthSession {
    AuthSession {
        source,
        session_id: "run".into(),
        user_id: "operator".into(),
        username: "operator".into(),
        role,
        started_at_epoch_ms: 0,
    }
}

#[test]
fn system_user_can_use_the_workspace_without_an_application_admin_role() {
    let actor = session(IdentitySource::SystemCurrentProcess, UserRole::User);
    let permissions = PermissionService::new(DebugPolicy { debug_build: false });
    for action in [
        PermissionAction::ReadFile,
        PermissionAction::ExecuteCommandLine,
        PermissionAction::ChangeSettings,
        PermissionAction::ManageLauncher,
    ] {
        assert!(permissions.authorize(Some(&actor), action, None).allowed);
        assert!(!permissions.authorize(None, action, None).allowed);
    }
    assert!(
        !permissions
            .authorize(Some(&actor), PermissionAction::EnterDebugMode, None)
            .allowed
    );
}

#[test]
fn desktop_system_identity_does_not_enable_user_management() {
    let actor = session(IdentitySource::SystemCurrentProcess, UserRole::Admin);
    let result =
        PermissionService::default().authorize(Some(&actor), PermissionAction::ManageUsers, None);
    assert_eq!(result.reason.as_deref(), Some("unsupported_platform"));
}
