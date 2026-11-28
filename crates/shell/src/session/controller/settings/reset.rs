use super::*;
use workspace_protocol::ResetTarget;

impl ShellSession {
    pub(super) fn request_settings_reset(&mut self, field: ui::SettingsField) {
        if !self.can_change_global_settings() {
            self.set_settings_error(i18n::msg!("settings-admin-required"));
            return;
        }
        let (target, title, body) = match field {
            ui::SettingsField::ResetAllSettings => {
                let Some(actor) = self.app.auth_session() else {
                    return;
                };
                (
                    ResetTarget::Settings {
                        username: actor.username.clone(),
                        user_id: actor.user_id.clone(),
                    },
                    i18n::msg!("settings-reset-all"),
                    i18n::msg!("settings-reset-all-confirm"),
                )
            }
            ui::SettingsField::ResetUserData => (
                ResetTarget::UserData,
                i18n::msg!("settings-reset-data"),
                i18n::msg!("settings-reset-data-confirm"),
            ),
            _ => return,
        };
        self.notify_modal_with_options(
            ShellNotification::modal(
                title,
                body,
                ui::NotificationTone::Warning,
                vec![
                    ShellNotificationAction::new("reset", i18n::msg!("settings-reset"))
                        .with_follow_up(ShellCommand::SettingsResetConfirmed(target)),
                    ShellNotificationAction::new("cancel", i18n::msg!("settings-cancel"))
                        .with_shortcut(InputKey::Escape)
                        .cancel(),
                ],
            )
            .for_session()
            .with_cancel_default(),
        );
    }

    pub(in crate::session) fn confirm_settings_reset(&mut self, target: ResetTarget) {
        if !self.can_request_settings_reset(&target) {
            self.set_settings_error(i18n::msg!("settings-admin-required"));
            return;
        }
        self.requested_reset = Some(target);
    }

    pub(in crate::session) fn can_request_settings_reset(&self, target: &ResetTarget) -> bool {
        self.can_change_global_settings()
            && match target {
                ResetTarget::Settings { username, user_id } => self
                    .app
                    .auth_session()
                    .is_some_and(|actor| &actor.username == username && &actor.user_id == user_id),
                ResetTarget::UserData => true,
            }
    }
}
