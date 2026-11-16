use super::*;
use crate::session_services::{
    SessionProfile, SessionServicesClient, SystemStatusRequests,
    client::{SyncState, service_error},
};
use workspace_protocol::{ServiceError, ServiceOperation, ServiceValue, WorkerRole};

type Completion = Box<dyn FnOnce(&mut ShellSession, io::Result<ServiceValue>) + Send>;

pub(crate) struct PendingWrite {
    receiver: std::sync::mpsc::Receiver<Result<ServiceValue, ServiceError>>,
    session: Option<Option<String>>,
    page: Option<PageInstanceId>,
    complete: Option<Completion>,
    task: Option<(u64, Arc<std::sync::atomic::AtomicBool>)>,
    clipboard: Option<u64>,
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
            clipboard: None,
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
                if let Some(token) = pending.clipboard {
                    self.complete_clipboard_token(token);
                }
                continue;
            }
            if pending.task.is_some()
                && result.is_err()
                && matches!(
                    state.window_close,
                    super::app_host::WindowClose::Checking
                        | super::app_host::WindowClose::Confirmed
                )
            {
                state.window_close = super::app_host::WindowClose::Cancelled;
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
            if let Some(token) = pending.clipboard {
                self.complete_clipboard_token(token);
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
                        clipboard: None,
                    });
            }
            return Ok(false);
        }
        let mut changed = false;
        if !shell && state.app.auth_session() != auth.1.as_ref() {
            state
                .app
                .dispatch_at(app::AppCommand::SetAuthSession(None), Instant::now());
            state.reset_workspace_clock_profile();
            sync.auth = Some((auth.0, None));
            sync.profile = None;
            return Ok(true);
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
                        clipboard: None,
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
                        clipboard: None,
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
            *result.lock().unwrap_or_else(|error| error.into_inner()) =
                Some(SessionServicesClient::in_authorization(revision, work));
            let _ = reply.send(Ok(ServiceValue::Empty));
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
                clipboard: None,
            });
        Ok(())
    }

    pub(super) fn submit_workspace_write(
        &mut self,
        operation: ServiceOperation,
        page: bool,
        complete: impl FnOnce(&mut Self, io::Result<ServiceValue>) + Send + 'static,
    ) -> io::Result<()> {
        self.submit_workspace_write_kind(operation, page, false, complete)
    }

    pub(super) fn submit_workspace_clipboard(
        &mut self,
        operation: ServiceOperation,
        complete: impl FnOnce(&mut Self, io::Result<ServiceValue>) + Send + 'static,
    ) -> io::Result<()> {
        self.submit_workspace_write_kind(operation, true, true, complete)
    }

    fn submit_workspace_write_kind(
        &mut self,
        operation: ServiceOperation,
        page: bool,
        clipboard: bool,
        complete: impl FnOnce(&mut Self, io::Result<ServiceValue>) + Send + 'static,
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
        let track = !matches!(
            operation,
            ServiceOperation::WriteClockTime { .. } | ServiceOperation::WriteSystemSnapshot { .. }
        );
        let receiver = client.submit(SessionServicesClient::authorized(operation, revision))?;
        let clipboard = clipboard.then(|| client.submit_clipboard_token());
        let task = track.then(|| self.register_workspace_save());
        client
            .completions
            .lock()
            .map_err(|_| service_error(ServiceError::Unavailable))?
            .push(PendingWrite {
                receiver,
                session: Some(session),
                page: page.then(|| self.active_page_id()),
                complete: Some(Box::new(complete)),
                task,
                clipboard,
            });
        Ok(())
    }

    pub(in crate::session) fn defer_workspace_config(
        &mut self,
        config: storage::StorageConfig,
        complete: impl FnOnce(&mut Self, io::Result<storage::StorageConfig>) + Send + 'static,
    ) -> bool {
        if SessionServicesClient::current().is_none() {
            return false;
        }
        let completion = Arc::new(std::sync::Mutex::new(Some(complete)));
        let callback = completion.clone();
        let auth_revision = config.session_authorization_revision;
        let operation = (|| -> io::Result<ServiceOperation> {
            let revision = config
                .session_revision
                .ok_or_else(|| io::Error::other("configuration was not read"))?;
            let client = SessionServicesClient::current().expect("services available");
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
            Ok(ServiceOperation::WriteConfig {
                revision,
                config: SessionServicesClient::encode(&config)?,
            })
        })();
        let result = operation.and_then(|operation| {
            self.submit_workspace_write(operation, true, move |state, result| {
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
            })
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
