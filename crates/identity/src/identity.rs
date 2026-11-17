use platform::{Platform, PlatformError, PlatformKind};

use crate::authorization::UserRole;
use crate::time::{unix_millis, unix_nanos};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum IdentitySource {
    SystemCurrentProcess,
    LinuxCurrentProcess,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AuthSession {
    pub source: IdentitySource,
    pub session_id: String,
    pub user_id: String,
    pub username: String,
    pub role: UserRole,
    pub started_at_epoch_ms: u64,
}

impl AuthSession {
    pub fn current(platform: &dyn Platform) -> Result<Self, PlatformError> {
        let username = platform.current_username()?;
        if username.is_empty() || username.chars().any(char::is_control) {
            return Err(PlatformError::InvalidInput {
                message: i18n::tr!("account-invalid-system-username"),
            });
        }
        let source = if platform.kind() == PlatformKind::Linux {
            IdentitySource::LinuxCurrentProcess
        } else {
            IdentitySource::SystemCurrentProcess
        };
        let role = UserRole::User;
        #[cfg(target_os = "linux")]
        let role = if source == IdentitySource::LinuxCurrentProcess
            && platform.is_native_backend()
            && platform::linux::accounts::Accounts::current_account()
                .is_ok_and(|account| account.admin)
        {
            UserRole::Admin
        } else {
            role
        };
        Ok(Self {
            source,
            session_id: format!("app-session-{}-{}", std::process::id(), unix_nanos()),
            user_id: username.clone(),
            username,
            role,
            started_at_epoch_ms: unix_millis(),
        })
    }
}
