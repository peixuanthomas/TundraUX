use super::{
    window_launch::{WindowApplication, WindowOwner},
    *,
};
use crate::session_services::{
    SessionServicesClient,
    clock::{ClockAnchor, SharedClockProfile},
};
use workspace_protocol::{ServiceOperation, WorkerRole};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct WorkspaceClock {
    profile: Option<(String, u64, ClockProfile)>,
    time: Option<u64>,
    publish: Option<ClockAnchor>,
    publish_pending: bool,
    pending: bool,
}

impl ShellSession {
    pub(super) fn reset_workspace_clock_profile(&mut self) {
        self.workspace_clock.profile = None;
        self.workspace_clock.pending = false;
        self.workspace_clock.publish_pending = false;
    }

    pub(super) fn publish_workspace_clock(&mut self, utc: Option<DateTime<Utc>>) {
        if self.window_owner == WindowOwner::Shell {
            self.workspace_clock.publish = Some(ClockAnchor {
                utc,
                observed_at: Utc::now(),
            });
        }
    }

    pub(super) fn queue_workspace_clock_profile(
        &mut self,
        previous: Option<ClockScheduler>,
        reproject: bool,
        snapshot: &time::ClockSnapshot,
        now: Instant,
        complete: impl FnOnce(&mut Self, Result<(), i18n::LocalizedText>) + Send + 'static,
    ) {
        let unavailable =
            || i18n::LocalizedText::from(i18n::msg!("workspace-services-unavailable"));
        let result = (|| {
            let (session, revision, stored) = self
                .workspace_clock
                .profile
                .as_ref()
                .ok_or_else(unavailable)?;
            if self.workspace_clock.pending
                || self
                    .app
                    .auth_session()
                    .is_none_or(|auth| &auth.session_id != session)
            {
                return Err(unavailable());
            }
            let scheduler = self.clock_scheduler.clone().ok_or_else(unavailable)?;
            let profile = match previous.as_ref().filter(|_| !reproject) {
                Some(previous) => {
                    scheduler.export_profile_mutation(previous, stored, snapshot, now)
                }
                None => scheduler.export_profile(snapshot, now),
            };
            let updated = SharedClockProfile {
                profile,
                alarm_dates: previous
                    .as_ref()
                    .map(|previous| scheduler.changed_alarm_dates(previous))
                    .unwrap_or_default(),
            };
            let operation = ServiceOperation::WriteClockProfile {
                revision: *revision,
                profile: SessionServicesClient::encode(&updated)
                    .map_err(|error| error.to_string())?,
            };
            Ok((operation, scheduler, updated))
        })();
        if let Some(previous) = previous {
            self.clock_scheduler = Some(previous);
        }
        let (operation, scheduler, updated) = match result {
            Ok(result) => result,
            Err(error) => {
                complete(self, Err(error));
                return;
            }
        };
        self.workspace_clock.pending = true;
        let completion = Arc::new(std::sync::Mutex::new(Some(complete)));
        let callback = completion.clone();
        let queued = self.submit_workspace_write(operation, false, move |state, result| {
            state.workspace_clock.pending = false;
            let result = result
                .and_then(|value| match value {
                    workspace_protocol::ServiceValue::Versioned { revision, .. } => Ok(revision),
                    workspace_protocol::ServiceValue::Empty => {
                        Err(io::ErrorKind::InvalidData.into())
                    }
                })
                .map(|revision| {
                    if let Some((_, current_revision, stored)) = &mut state.workspace_clock.profile
                    {
                        *current_revision = revision;
                        *stored = updated.profile;
                    }
                    state.clock_scheduler = Some(scheduler);
                })
                .map_err(|error| i18n::LocalizedText::from(error.to_string()));
            if let Some(complete) = callback
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .take()
            {
                complete(state, result);
            }
        });
        if let Err(error) = queued {
            self.workspace_clock.pending = false;
            if let Some(complete) = completion
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .take()
            {
                complete(self, Err(error.to_string().into()));
            }
        }
    }

    pub(super) fn advance_workspace_clock(
        &mut self,
        _client: &SessionServicesClient,
        snapshot: &time::ClockSnapshot,
        now: Instant,
    ) -> bool {
        if self.workspace_clock.profile.is_none() || self.workspace_clock.pending {
            return false;
        }
        let Some(scheduler) = self.clock_scheduler.as_mut() else {
            return false;
        };
        let previous = scheduler.clone();
        let due = scheduler.advance(snapshot, now);
        let reproject = self.clock_persist_pending;
        if due.is_empty() && !reproject {
            return false;
        }
        self.queue_workspace_clock_profile(
            Some(previous),
            reproject,
            snapshot,
            now,
            move |state, result| {
                if let Err(error) = result {
                    state.report_clock_storage_error(error);
                    return;
                }
                state.clock_persist_pending = false;
                state.sync_clock_selection_at(Instant::now());
                let _ = state.handle_clock_due_events(due);
                state.resolve_notification_alert(CLOCK_STORAGE_ALERT_KEY);
                state.refresh_hit_map();
            },
        );
        false
    }
}

pub(super) fn sync_clock_workspace(
    client: &SessionServicesClient,
    state: &mut ShellSession,
) -> io::Result<bool> {
    let previous_profile = state
        .workspace_clock
        .profile
        .as_ref()
        .map(|(session, revision, _)| (session.clone(), *revision));
    let previous_time = state.workspace_clock.time;
    let shell = client.worker().role == WorkerRole::Shell;
    if shell {
        if let Some(anchor) = state.workspace_clock.publish
            && !state.workspace_clock.publish_pending
        {
            state.submit_workspace_write(
                ServiceOperation::WriteClockTime {
                    anchor: SessionServicesClient::encode(&anchor)?,
                },
                false,
                move |state, result| {
                    state.workspace_clock.publish_pending = false;
                    if result.is_ok() && state.workspace_clock.publish == Some(anchor) {
                        state.workspace_clock.publish = None;
                    }
                },
            )?;
            state.workspace_clock.publish_pending = true;
        }
    } else if state.window_owner == WindowOwner::App(WindowApplication::Clock) {
        let (revision, anchor): (_, Option<ClockAnchor>) =
            client.read(ServiceOperation::ReadClockTime)?;
        if let Some(anchor) = anchor
            && state.workspace_clock.time != Some(revision)
        {
            state.workspace_clock.time = Some(revision);
            state.time_sync_attempted = true;
            if let Some(utc) = anchor.current_utc() {
                state.last_time_sync_utc = Some(utc);
                state
                    .app
                    .dispatch_at(app::AppCommand::ApplyTimeSync(Ok(utc)), Instant::now());
            }
        }
    } else {
        return Ok(previous_profile
            != state
                .workspace_clock
                .profile
                .as_ref()
                .map(|(session, revision, _)| (session.clone(), *revision))
            || previous_time != state.workspace_clock.time);
    }
    let Some(auth) = state
        .app
        .auth_session()
        .filter(|auth| auth.role != UserRole::Guest)
    else {
        state.workspace_clock.profile = None;
        state.clock_scheduler = None;
        state.clock_profile_pending_sync = None;
        state.pages.clock.clock_create_state = None;
        return Ok(previous_profile
            != state
                .workspace_clock
                .profile
                .as_ref()
                .map(|(session, revision, _)| (session.clone(), *revision))
            || previous_time != state.workspace_clock.time);
    };
    let session = auth.session_id.clone();
    if state
        .workspace_clock
        .profile
        .as_ref()
        .is_some_and(|(old, _, _)| old != &session)
    {
        state.workspace_clock.profile = None;
        state.clock_scheduler = None;
        state.pages.clock.clock_create_state = None;
    }
    if state.workspace_clock.pending {
        return Ok(previous_time != state.workspace_clock.time);
    }
    let (
        revision,
        SharedClockProfile {
            profile,
            alarm_dates,
        },
    ): (_, SharedClockProfile) = client.read(ServiceOperation::ReadClockProfile)?;
    if state
        .workspace_clock
        .profile
        .as_ref()
        .is_some_and(|(_, old, _)| *old == revision)
    {
        return Ok(previous_profile
            != state
                .workspace_clock
                .profile
                .as_ref()
                .map(|(session, revision, _)| (session.clone(), *revision))
            || previous_time != state.workspace_clock.time);
    }
    if !state.time_sync_attempted && !profile.entries.is_empty() {
        state.clock_profile_pending_sync = Some(profile);
        return Ok(previous_profile
            != state
                .workspace_clock
                .profile
                .as_ref()
                .map(|(session, revision, _)| (session.clone(), *revision))
            || previous_time != state.workspace_clock.time);
    }
    state.clock_profile_pending_sync = None;
    let snapshot = state.app.snapshot().clock;
    let now = Instant::now();
    let previous = state
        .ui
        .workspace_clock
        .profile
        .as_ref()
        .map(|(_, _, profile)| profile);
    state
        .ui
        .clock_scheduler
        .get_or_insert_with(ClockScheduler::default)
        .synchronize_profile(previous, &profile, &alarm_dates, &snapshot, now);
    state.workspace_clock.profile = Some((session, revision, profile));
    state.sync_clock_selection_at(now);
    state.resolve_notification_alert(CLOCK_STORAGE_ALERT_KEY);
    Ok(previous_profile
        != state
            .workspace_clock
            .profile
            .as_ref()
            .map(|(session, revision, _)| (session.clone(), *revision))
        || previous_time != state.workspace_clock.time)
}
