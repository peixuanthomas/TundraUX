use super::*;
use crate::session_services::{
    SessionProfile, SessionServicesClient, SystemStatusRequests,
    client::{SyncState, service_error},
};
use workspace_protocol::{ServiceError, ServiceOperation, ServiceValue, WorkerRole};

type Completion = Box<dyn FnOnce(&mut ShellSession, io::Result<ServiceValue>) + Send>;

fn automation_configuration_changed(
    config: &storage::StorageConfig,
    current: &workspace_protocol::ServiceDomain,
) -> io::Result<bool> {
    let current = current
        .as_ref()
        .map_err(|error| service_error(error.clone()))?;
    let enabled = current
        .value
        .get("automation")
        .and_then(|automation| automation.get("enabled"))
        .and_then(serde_json::Value::as_bool)
        .ok_or(io::ErrorKind::InvalidData)?;
    Ok(config.automation.enabled != enabled)
}

fn workspace_job_reply<T>(
    result: T,
    successful: impl FnOnce(&T) -> bool,
    outcome: &std::sync::Mutex<Option<T>>,
) -> Result<ServiceValue, ServiceError> {
    let success = successful(&result);
    *outcome.lock().unwrap_or_else(|error| error.into_inner()) = Some(result);
    if success {
        Ok(ServiceValue::Empty)
    } else {
        Err(ServiceError::Unavailable)
    }
}

pub(crate) struct PendingWrite {
    receiver: std::sync::mpsc::Receiver<Result<ServiceValue, ServiceError>>,
    session: Option<Option<String>>,
    page: Option<PageInstanceId>,
    complete: Option<Completion>,
    task: Option<(u64, Arc<std::sync::atomic::AtomicBool>)>,
    input_effect: Option<u64>,
    automation_authorization: Option<u64>,
}

impl PendingWrite {
    pub(crate) fn is_write(&self) -> bool {
        self.task.is_some()
    }
    pub(crate) fn silent(
        receiver: std::sync::mpsc::Receiver<Result<ServiceValue, ServiceError>>,
    ) -> Self {
        Self {
            receiver,
            session: None,
            page: None,
            complete: None,
            task: None,
            input_effect: None,
            automation_authorization: None,
        }
    }
}

impl SessionServicesClient {
    pub(crate) fn is_session_ready(&self, state: &ShellSession) -> bool {
        let Ok((_, failed, snapshot)) = self.snapshot() else {
            return false;
        };
        if failed {
            return false;
        }
        let Ok((_, auth)) = Self::decode::<Option<AuthSession>>(&snapshot.authorization) else {
            return false;
        };
        if auth.as_ref() != state.app.auth_session() {
            return false;
        }
        if auth
            .as_ref()
            .is_none_or(|auth| auth.role == UserRole::Guest)
        {
            return true;
        }
        Self::decode::<SessionProfile>(&snapshot.profile).is_ok_and(|(_, profile)| {
            profile.appearance == state.app.active_appearance().cloned()
                && profile.dashboard == state.app.active_system_status_dashboard().cloned()
        })
    }

    pub(crate) fn sync_session(&self, state: &mut ShellSession) -> bool {
        self.acknowledge_updates();
        let Ok((generation, failed, snapshot)) = self.snapshot() else {
            return false;
        };
        let mut changed = false;
        let mut completed = Vec::new();
        if let Ok(mut pending) = self.completions.try_lock() {
            let mut index = 0;
            while index < pending.len() {
                let result = match pending[index].receiver.try_recv() {
                    Ok(result) => Some(result.map_err(service_error)),
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        Some(Err(service_error(ServiceError::Unavailable)))
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => None,
                };
                if let Some(result) = result {
                    completed.push((pending.remove(index), result));
                } else {
                    index += 1;
                }
            }
        }
        for (pending, result) in completed {
            if let Some((request, finished)) = &pending.task {
                finished.store(true, std::sync::atomic::Ordering::Release);
                state
                    .tasks
                    .complete(TaskDomain::WorkspaceServices, *request);
            }
            let current = state.app.auth_session().map(|auth| auth.session_id.clone());
            if pending
                .session
                .as_ref()
                .is_some_and(|session| session != &current)
            {
                if let Some(token) = pending.input_effect {
                    self.complete_input_effect_token(token);
                }
                continue;
            }
            if pending.task.is_some() && result.is_err() {
                state.cancel_window_close_after_workspace_failure();
            }
            // A persisted global setting survives its originating page closing.
            if let (Some(authorization_revision), Ok(ServiceValue::Versioned { revision, value })) =
                (pending.automation_authorization, &result)
                && snapshot
                    .authorization
                    .as_ref()
                    .is_ok_and(|auth| auth.revision == authorization_revision)
                && let Some(enabled) = value
                    .get("automation")
                    .and_then(|automation| automation.get("enabled"))
                    .and_then(serde_json::Value::as_bool)
            {
                state.record_automation_commit(workspace_protocol::AutomationCommit {
                    enabled,
                    revision: *revision,
                    authorization_revision,
                    authorization_session: current.clone(),
                });
                changed = true;
            }
            if let Some(complete) = pending.complete {
                if let Some(page) = pending.page {
                    if state.page_screen(page).is_some() {
                        state.with_page_context(page, |state| complete(state, result));
                        changed = true;
                    } else if let Err(error) = result {
                        state.notify_status(error.to_string());
                        changed = true;
                    }
                } else {
                    complete(state, result);
                    changed = true;
                }
            } else if result.is_err() {
                state.notify_status(i18n::msg!("workspace-services-unavailable"));
                changed = true;
            }
            if let Some(token) = pending.input_effect {
                self.complete_input_effect_token(token);
            }
        }
        let Ok(mut sync) = self.sync.try_lock() else {
            return changed;
        };
        if failed && !sync.failed {
            state.notify_status(i18n::msg!("workspace-services-unavailable"));
            changed = true;
        }
        sync.failed |= failed;
        sync.generation = generation;
        match self.try_sync_session(state, &mut sync, &snapshot) {
            Ok(applied) => {
                sync.failed = failed;
                changed |= applied;
            }
            Err(_) if !sync.failed => {
                sync.failed = true;
                state.notify_status(i18n::msg!("workspace-services-unavailable"));
                changed = true;
            }
            Err(_) => {}
        }
        if changed {
            state.refresh_hit_map();
        }
        changed
    }

    fn try_sync_session(
        &self,
        state: &mut ShellSession,
        sync: &mut SyncState,
        snapshot: &workspace_protocol::ServiceSessionSnapshot,
    ) -> io::Result<bool> {
        let shell = self.worker().role == WorkerRole::Shell;
        let auth: (u64, Option<AuthSession>) = Self::decode(&snapshot.authorization)?;
        if shell && state.app.auth_session() != auth.1.as_ref() {
            if !sync.pending_authorization {
                let desired = state.app.auth_session().cloned();
                let receiver = self.submit(ServiceOperation::WriteAuthorization {
                    revision: auth.0,
                    policy: Self::encode(&desired)?,
                })?;
                sync.pending_authorization = true;
                self.completions
                    .lock()
                    .map_err(|_| service_error(ServiceError::Unavailable))?
                    .push(PendingWrite {
                        receiver,
                        session: None,
                        page: None,
                        complete: Some(Box::new(|_, _| {
                            if let Some(client) = SessionServicesClient::current()
                                && let Ok(mut sync) = client.sync.lock()
                            {
                                sync.pending_authorization = false;
                                sync.generation = 0;
                            }
                        })),
                        task: None,
                        input_effect: None,
                        automation_authorization: None,
                    });
            }
            return Ok(false);
        }
        let mut changed = false;
        if !shell && state.app.auth_session() != auth.1.as_ref() {
            let current = state.app.auth_session().map(|auth| &auth.session_id);
            let next = auth
                .1
                .as_ref()
                .filter(|next| Some(&next.session_id) == current)
                .cloned();
            state.app.dispatch_at(
                app::AppCommand::SetAuthSession(next.clone()),
                Instant::now(),
            );
            if next.is_none() {
                state.reset_workspace_clock_profile();
                sync.auth = Some((auth.0, None));
                sync.profile = None;
                return Ok(true);
            }
            changed = true;
        }
        if sync
            .auth
            .as_ref()
            .and_then(|(_, auth)| auth.as_ref())
            .map(|auth| &auth.session_id)
            != auth.1.as_ref().map(|auth| &auth.session_id)
        {
            sync.profile = None;
        }
        sync.auth = Some(auth.clone());
        let (_, mut config): (_, storage::StorageConfig) = Self::decode(&snapshot.config)?;
        config.session_revision = Some(
            snapshot
                .config
                .as_ref()
                .map_err(|error| service_error(error.clone()))?
                .revision,
        );
        config.session_authorization_revision = Some(auth.0);
        if state.identity_backend == identity::IdentityBackend::Linux {
            config.time_sync.source = storage::TimeSyncSource::OperatingSystem;
            config.time_sync.server_url = None;
        }
        if state.app.storage_config() != &config {
            let launcher_previous = (state.app.storage_config().launcher != config.launcher)
                .then(|| state.app.storage_config().launcher.clone());
            if state.language_code() != config.language
                && let Ok((catalog, loaded)) = state.prepare_language(&config.language)
            {
                state.publish_language(catalog, loaded);
            }
            state.replace_storage_config(config);
            if let Some(previous) = launcher_previous {
                state.sync_launcher_entries(&previous);
            }
            changed = true;
        }
        if let Ok(profile) = Self::decode::<SessionProfile>(&snapshot.profile) {
            let local = SessionProfile {
                appearance: state.app.active_appearance().cloned(),
                dashboard: state.app.active_system_status_dashboard().cloned(),
            };
            if shell
                && profile.1 == SessionProfile::default()
                && local.appearance.is_some()
                && local.dashboard.is_some()
                && !sync.pending_profile
            {
                let receiver = self.submit(Self::authorized(
                    ServiceOperation::WriteProfile {
                        revision: profile.0,
                        profile: Self::encode(&local)?,
                    },
                    auth.0,
                ))?;
                sync.pending_profile = true;
                self.completions
                    .lock()
                    .map_err(|_| service_error(ServiceError::Unavailable))?
                    .push(PendingWrite {
                        receiver,
                        session: None,
                        page: None,
                        complete: Some(Box::new(|_, _| {
                            if let Some(client) = SessionServicesClient::current()
                                && let Ok(mut sync) = client.sync.lock()
                            {
                                sync.pending_profile = false;
                            }
                        })),
                        task: None,
                        input_effect: None,
                        automation_authorization: None,
                    });
            }
            if local != profile.1 && profile.1 != SessionProfile::default() {
                state.apply_workspace_profile(profile.1.clone());
                changed = true;
            }
            sync.profile = Some(profile);
        }
        if shell {
            if let Some(snapshot) = state.app.system_status_snapshot()
                && sync.published_system_revision != Some(snapshot.revision)
                && sync.pending_system_revision != Some(snapshot.revision)
            {
                let revision = snapshot.revision;
                let receiver = self.submit(Self::authorized(
                    ServiceOperation::WriteSystemSnapshot {
                        snapshot: Self::encode(snapshot)?,
                    },
                    auth.0,
                ))?;
                sync.pending_system_revision = Some(revision);
                self.completions
                    .lock()
                    .map_err(|_| service_error(ServiceError::Unavailable))?
                    .push(PendingWrite {
                        receiver,
                        session: None,
                        page: None,
                        task: None,
                        input_effect: None,
                        automation_authorization: None,
                        complete: Some(Box::new(move |_, result| {
                            if let Some(client) = SessionServicesClient::current()
                                && let Ok(mut sync) = client.sync.lock()
                            {
                                if sync.pending_system_revision == Some(revision) {
                                    sync.pending_system_revision = None;
                                }
                                if result.is_ok() {
                                    sync.published_system_revision = Some(revision);
                                }
                            }
                        })),
                    });
            }
            let (_, requests): (_, SystemStatusRequests) = Self::decode(&snapshot.system_requests)?;
            if requests.active != sync.system_active {
                let _ = state
                    .settings_task_runtime
                    .set_system_status_active(requests.active);
                sync.system_active = requests.active;
            }
            if self.take_system_refresh() {
                let _ = state.settings_task_runtime.refresh_system_status();
            }
        } else if let Ok((revision, snapshot)) =
            Self::decode::<Option<app::AppSystemStatusSnapshot>>(&snapshot.system)
            && revision != sync.system_revision
        {
            if let Some(snapshot) = snapshot {
                changed |= state.apply_system_status_snapshot(snapshot);
            }
            sync.system_revision = revision;
        }
        if state.app.explorer_state().is_some()
            && let Ok(files) = &snapshot.files
            && sync.files_revision != Some(files.revision)
        {
            state
                .app
                .refresh_shared_explorer()
                .map_err(io::Error::other)?;
            sync.files_revision = Some(files.revision);
            changed = true;
        }
        changed |= super::clock_workspace::sync_clock_workspace(self, state)?;
        Ok(changed)
    }
}

impl ShellSession {
    pub(super) fn record_automation_commit(
        &mut self,
        commit: workspace_protocol::AutomationCommit,
    ) {
        if commit.authorization_session.as_deref()
            != self.app.auth_session().map(|auth| auth.session_id.as_str())
            || self
                .app
                .storage_config()
                .session_authorization_revision
                .is_some_and(|revision| revision > commit.authorization_revision)
        {
            return;
        }
        if self.automation_commit.as_ref().is_none_or(|previous| {
            previous.authorization_revision < commit.authorization_revision
                || (previous.authorization_revision == commit.authorization_revision
                    && previous.revision <= commit.revision)
        }) {
            self.automation_commit = Some(commit);
        }
    }

    fn cancel_window_close_after_workspace_failure(&mut self) {
        if !self.window_close_explicit
            && matches!(
                self.window_close,
                super::app_host::WindowClose::Checking | super::app_host::WindowClose::Confirmed
            )
        {
            self.window_close = super::app_host::WindowClose::Cancelled;
        }
    }

    pub(in crate::session) fn begin_workspace_draft(&mut self) {
        self.pages.common.workspace_draft = self.pages.common.workspace_draft.wrapping_add(1);
    }

    fn register_workspace_save(&mut self) -> (u64, Arc<std::sync::atomic::AtomicBool>) {
        static NEXT_REQUEST: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let request = NEXT_REQUEST.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let finished = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let completed = finished.clone();
        self.register_page_task(
            TaskDomain::WorkspaceServices,
            request,
            TaskClass::Operation,
            TaskControl::new(
                || {},
                move || completed.load(std::sync::atomic::Ordering::Acquire),
            )
            .without_history(),
        );
        (request, finished)
    }

    pub(in crate::session) fn submit_workspace_job<T: Send + 'static>(
        &mut self,
        work: impl FnOnce() -> T + Send + 'static,
        successful: impl FnOnce(&T) -> bool + Send + 'static,
        complete: impl FnOnce(&mut Self, T) + Send + 'static,
    ) -> io::Result<()> {
        let client = SessionServicesClient::current()
            .ok_or_else(|| service_error(ServiceError::Unavailable))?;
        let (_, _, snapshot) = client.snapshot()?;
        let (revision, authorization): (_, Option<AuthSession>) =
            SessionServicesClient::decode(&snapshot.authorization)?;
        let session = self.app.auth_session().map(|auth| auth.session_id.clone());
        if session.as_deref() != authorization.as_ref().map(|auth| auth.session_id.as_str()) {
            return Err(service_error(ServiceError::Forbidden));
        }
        let outcome = Arc::new(std::sync::Mutex::new(None));
        let result = outcome.clone();
        let (reply, receiver) = std::sync::mpsc::sync_channel(1);
        let completion_client = client.clone();
        let language = self.language.clone();
        client.submit_job(Box::new(move || {
            let _language = i18n::enter_snapshot(language);
            let value = SessionServicesClient::in_authorization(revision, work);
            let _ = reply.send(workspace_job_reply(value, successful, &result));
            completion_client.notify_completion();
        }))?;
        let task = Some(self.register_workspace_save());
        client
            .completions
            .lock()
            .map_err(|_| service_error(ServiceError::Unavailable))?
            .push(PendingWrite {
                receiver,
                session: Some(session),
                page: Some(self.active_page_id()),
                complete: Some(Box::new(move |state, _| {
                    if let Some(result) = outcome
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .take()
                    {
                        complete(state, result);
                    }
                })),
                task,
                input_effect: None,
                automation_authorization: None,
            });
        Ok(())
    }

    pub(super) fn submit_workspace_write(
        &mut self,
        operation: ServiceOperation,
        page: bool,
        complete: impl FnOnce(&mut Self, io::Result<ServiceValue>) + Send + 'static,
    ) -> io::Result<()> {
        let client = SessionServicesClient::current()
            .ok_or_else(|| service_error(ServiceError::Unavailable))?;
        self.submit_workspace_write_kind(client, operation, page, false, None, complete)
    }

    pub(super) fn submit_workspace_clipboard(
        &mut self,
        operation: ServiceOperation,
        complete: impl FnOnce(&mut Self, io::Result<ServiceValue>) + Send + 'static,
    ) -> io::Result<()> {
        let client = SessionServicesClient::current()
            .ok_or_else(|| service_error(ServiceError::Unavailable))?;
        self.submit_workspace_write_kind(client, operation, true, true, None, complete)
    }

    fn submit_workspace_write_kind(
        &mut self,
        client: &SessionServicesClient,
        operation: ServiceOperation,
        page: bool,
        input_effect: bool,
        expected_authorization: Option<u64>,
        complete: impl FnOnce(&mut Self, io::Result<ServiceValue>) + Send + 'static,
    ) -> io::Result<()> {
        let (_, _, snapshot) = client.snapshot()?;
        let (revision, authorization): (_, Option<AuthSession>) =
            SessionServicesClient::decode(&snapshot.authorization)?;
        if expected_authorization.is_some_and(|expected| expected != revision) {
            return Err(service_error(ServiceError::Forbidden));
        }
        let session = self.app.auth_session().map(|auth| auth.session_id.clone());
        if session.as_deref() != authorization.as_ref().map(|auth| auth.session_id.as_str()) {
            return Err(service_error(ServiceError::Forbidden));
        }
        let track = !matches!(
            operation,
            ServiceOperation::WriteClockTime { .. } | ServiceOperation::WriteSystemSnapshot { .. }
        );
        let mut pending = client
            .completions
            .lock()
            .map_err(|_| service_error(ServiceError::Unavailable))?;
        let receiver = client.submit(SessionServicesClient::authorized(
            operation,
            expected_authorization.unwrap_or(revision),
        ))?;
        let automation_authorization = input_effect.then_some(expected_authorization).flatten();
        let input_effect = input_effect.then(|| client.submit_input_effect_token());
        let task = track.then(|| self.register_workspace_save());
        pending.push(PendingWrite {
            receiver,
            session: Some(session),
            page: page.then(|| self.active_page_id()),
            complete: Some(Box::new(complete)),
            task,
            input_effect,
            automation_authorization,
        });
        Ok(())
    }

    pub(in crate::session) fn defer_workspace_config(
        &mut self,
        config: storage::StorageConfig,
        complete: impl FnOnce(&mut Self, io::Result<storage::StorageConfig>) + Send + 'static,
    ) -> bool {
        let Some(client) = SessionServicesClient::current() else {
            return false;
        };
        let completion = Arc::new(std::sync::Mutex::new(Some(complete)));
        let callback = completion.clone();
        let auth_revision = config.session_authorization_revision;
        let operation = (|| -> io::Result<(ServiceOperation, bool)> {
            let revision = config
                .session_revision
                .ok_or_else(|| io::Error::other("configuration was not read"))?;
            let (_, _, snapshot) = client.snapshot()?;
            if snapshot
                .authorization
                .as_ref()
                .ok()
                .map(|auth| auth.revision)
                != auth_revision
            {
                return Err(service_error(ServiceError::Forbidden));
            }
            let confirm_automation = automation_configuration_changed(&config, &snapshot.config)?;
            Ok((
                ServiceOperation::WriteConfig {
                    revision,
                    config: SessionServicesClient::encode(&config)?,
                },
                confirm_automation,
            ))
        })();
        let result = operation.and_then(|(operation, confirm_automation)| {
            self.submit_workspace_write_kind(
                client,
                operation,
                true,
                confirm_automation,
                auth_revision,
                move |state, result| {
                    let result = result.and_then(|value| match value {
                        ServiceValue::Versioned { revision, value } => {
                            let mut config: storage::StorageConfig = serde_json::from_value(value)?;
                            config.session_revision = Some(revision);
                            config.session_authorization_revision = auth_revision;
                            Ok(config)
                        }
                        ServiceValue::Empty => Err(io::ErrorKind::InvalidData.into()),
                    });
                    if let Some(complete) = callback
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .take()
                    {
                        complete(state, result);
                    }
                },
            )
        });
        if let Err(error) = result {
            if let Some(complete) = completion
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .take()
            {
                complete(self, Err(error));
            }
        }
        true
    }

    pub(super) fn workspace_profile_revision(&self) -> Option<u64> {
        let services = SessionServicesClient::current()?;
        let sync = services.sync.lock().ok()?;
        sync.profile.as_ref().map(|(revision, _)| *revision)
    }

    pub(super) fn apply_workspace_profile(&mut self, profile: SessionProfile) {
        let now = Instant::now();
        self.app.dispatch_at(
            app::AppCommand::SetActiveAppearance(profile.appearance),
            now,
        );
        self.app.dispatch_at(
            app::AppCommand::SetActiveSystemStatusDashboard(profile.dashboard),
            now,
        );
    }
}

#[cfg(test)]
#[path = "../../tests/unit/session/services_client.rs"]
mod tests;
