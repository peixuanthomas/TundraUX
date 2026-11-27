use crate::session::*;

impl ShellSession {
    /// Polls background work without routing a synthetic input or cloning page state.
    pub(in crate::session) fn poll_background_at(
        &mut self,
        platform: &dyn Platform,
        now: Instant,
    ) -> (ShellAction, bool) {
        self.tick_count = self.tick_count.saturating_add(1);
        let mut changed = self.notification_expire(now);
        changed |= self.advance_screen_saver_at(now);
        changed |= self.advance_clock_background();
        changed |= self.poll_explorer_background_tasks(platform);
        changed |= self.poll_launcher_background_tasks();
        changed |= self.poll_settings_background_tasks();
        changed |= self.drain_diagnostics_events();
        if self.pending_session_exit.is_some() {
            self.drain_logs_for_shutdown();
            self.drain_management_for_shutdown();
        } else {
            changed |= self.poll_logs_tasks();
            changed |= self.poll_management();
            if !self.window_owner.is_app() {
                changed |= self.poll_package_launcher();
            }
        }
        changed |= self.poll_auto_admin();
        changed |= self.poll_editor_background_tasks(platform);
        changed |= self.persist_all_editor_recovery_if_due(now);
        changed |= self.poll_page_closes();
        if !self.tasks.has_running()
            && let Some(command) = self.pending_session_exit.take()
        {
            let action = self.apply_routed_event_once(
                RoutedEvent {
                    command,
                    input: InputEvent::Tick,
                    target: RoutedTarget::Global,
                },
                platform,
                now,
            );
            return (action, true);
        }
        if changed {
            self.finish_modal_focus_transition();
        }
        (ShellAction::Redraw, changed)
    }

    pub(in crate::session) fn background_timer_timeout(
        &self,
        now: Instant,
        maximum: Duration,
    ) -> Duration {
        let mut timeout = self.logs_poll_timeout(now, maximum);
        timeout = self.management_poll_timeout(now, timeout);
        timeout = self
            .editor_task_runtime
            .recovery_cleanup_poll_timeout(now, timeout);
        if self.storage_manager.is_some() && self.app.auth_session().is_some() {
            for (id, page) in self.pages.editor.iter() {
                if page.editor_config.path.is_some()
                    || page.editor_read_session.is_some()
                    || self.app.editor_state_for_page(id).is_none_or(|state| {
                        !state.is_dirty()
                            || !state.can_save()
                            || state
                                .document
                                .path
                                .as_deref()
                                .is_some_and(Self::is_system_config_path)
                    })
                {
                    continue;
                }
                let Some(dirty) = page.editor_recovery_dirty_since else {
                    return Duration::ZERO;
                };
                let mut at = dirty + super::editor::EDITOR_RECOVERY_IDLE;
                if let Some(last) = page.editor_last_recovery_write {
                    at = at.max(last + super::editor::EDITOR_RECOVERY_INTERVAL);
                }
                if let Some(retry_at) = page.editor_recovery_retry_after {
                    at = at.max(retry_at);
                }
                timeout = timeout.min(at.saturating_duration_since(now));
            }
        }
        timeout
    }
}
