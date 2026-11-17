use std::fmt;

use serde::{Deserialize, Serialize};

use crate::identity::AuthSession;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UserRole {
    Guest,
    User,
    Admin,
}

impl UserRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Guest => "Guest",
            Self::User => "User",
            Self::Admin => "Admin",
        }
    }
}

impl fmt::Display for UserRole {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PermissionAction {
    ReadFile,
    WriteFile,
    DeleteFile,
    MoveFile,
    OpenExternal,
    ExecuteCommandLine,
    ManageLauncher,
    ManageUsers,
    ViewDiagnosticsDetails,
    RepairDiagnostics,
    ChangeSettings,
    EnterDebugMode,
}

impl PermissionAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReadFile => "ReadFile",
            Self::WriteFile => "WriteFile",
            Self::DeleteFile => "DeleteFile",
            Self::MoveFile => "MoveFile",
            Self::OpenExternal => "OpenExternal",
            Self::ExecuteCommandLine => "ExecuteCommandLine",
            Self::ManageLauncher => "ManageLauncher",
            Self::ManageUsers => "ManageUsers",
            Self::ViewDiagnosticsDetails => "ViewDiagnosticsDetails",
            Self::RepairDiagnostics => "RepairDiagnostics",
            Self::ChangeSettings => "ChangeSettings",
            Self::EnterDebugMode => "EnterDebugMode",
        }
    }
}

impl fmt::Display for PermissionAction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Authorization {
    pub allowed: bool,
    pub reason: Option<String>,
}

impl Authorization {
    pub fn allow() -> Self {
        Self {
            allowed: true,
            reason: None,
        }
    }

    pub fn deny(reason: impl Into<String>) -> Self {
        Self {
            allowed: false,
            reason: Some(reason.into()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DebugPolicy {
    pub debug_build: bool,
}

impl DebugPolicy {
    pub fn current_build() -> Self {
        Self {
            debug_build: cfg!(debug_assertions),
        }
    }

    pub const fn allows_debug(self) -> bool {
        self.debug_build
    }
}

impl Default for DebugPolicy {
    fn default() -> Self {
        Self::current_build()
    }
}

#[derive(Debug, Clone)]
pub struct PermissionService {
    debug_policy: DebugPolicy,
}

impl PermissionService {
    pub fn new(debug_policy: DebugPolicy) -> Self {
        Self { debug_policy }
    }

    pub fn authorize(
        &self,
        session: Option<&AuthSession>,
        action: PermissionAction,
        _resource: Option<&str>,
    ) -> Authorization {
        let Some(session) = session.filter(|session| session.role != UserRole::Guest) else {
            return Authorization::deny("not_authenticated");
        };
        match action {
            PermissionAction::ManageUsers
                if session.source != crate::IdentitySource::LinuxCurrentProcess =>
            {
                Authorization::deny("unsupported_platform")
            }
            PermissionAction::ManageUsers if session.role != UserRole::Admin => {
                Authorization::deny("insufficient_role")
            }
            PermissionAction::EnterDebugMode if !self.debug_policy.allows_debug() => {
                Authorization::deny("debug_policy_denied")
            }
            _ => Authorization::allow(),
        }
    }
}

impl Default for PermissionService {
    fn default() -> Self {
        Self::new(DebugPolicy::default())
    }
}
