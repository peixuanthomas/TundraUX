use super::{SessionProfile, client_jobs::ServiceJobs};
use crate::process_host::WorkerConnection;
use app::explorer::{
    ExplorerError, ExplorerSharedOperations, ExplorerSharedRequest, ExplorerSharedSnapshot,
};
use serde::{Serialize, de::DeserializeOwned};
use std::io;
use std::sync::{
    Arc, Mutex, OnceLock,
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc,
};
use std::time::Duration;
use workspace_protocol::{
    ServiceDomain, ServiceError, ServiceOperation, ServiceSessionSnapshot, ServiceValue, WorkerId,
};

static CLIENT: OnceLock<Arc<SessionServicesClient>> = OnceLock::new();
thread_local! {
    static JOB_AUTHORIZATION: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
}
pub(super) const REQUEST_TIMEOUT: Duration = Duration::from_millis(200);
pub(super) type Reply = Result<ServiceValue, ServiceError>;

pub(super) struct Request {
    pub operation: ServiceOperation,
    pub reply: mpsc::SyncSender<Reply>,
}

pub(super) struct SnapshotState {
    pub snapshot: Arc<ServiceSessionSnapshot>,
    pub generation: u64,
    pub failed: bool,
}

pub(super) struct Shared {
    pub snapshot: Mutex<Option<SnapshotState>>,
    pub updated: AtomicBool,
    pub system_refresh: AtomicBool,
}

#[derive(Default)]
pub(crate) struct SyncState {
    pub auth: Option<(u64, Option<identity::AuthSession>)>,
    pub profile: Option<(u64, SessionProfile)>,
    pub generation: u64,
    pub system_revision: u64,
    pub files_revision: Option<u64>,
    pub published_system_revision: Option<u64>,
    pub pending_system_revision: Option<u64>,
    pub failed: bool,
    pub system_active: bool,
    pub pending_authorization: bool,
    pub pending_profile: bool,
}

pub(crate) struct SessionServicesClient {
    worker: WorkerId,
    commands: tokio::sync::mpsc::Sender<Request>,
    shared: Arc<Shared>,
    jobs: Option<ServiceJobs>,
    _rpc: Option<watchdog::OwnedTask<()>>,
    clipboard_submitted: AtomicU64,
    clipboard_completed: Mutex<ClipboardCompletions>,
    pub sync: Mutex<SyncState>,
    pub(crate) completions: Mutex<Vec<crate::session::services_client::PendingWrite>>,
}

#[derive(Default)]
struct ClipboardCompletions {
    watermark: u64,
    pending: std::collections::BTreeSet<u64>,
}

impl std::fmt::Debug for SessionServicesClient {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SessionServicesClient")
            .field("worker", &self.worker)
            .finish_non_exhaustive()
    }
}

impl SessionServicesClient {
    #[cfg(test)]
    pub(crate) fn for_test(snapshot: ServiceSessionSnapshot) -> Arc<Self> {
        let (commands, _) = tokio::sync::mpsc::channel(1);
        Arc::new(Self {
            worker: WorkerId {
                run: workspace_protocol::RunId(1),
                role: workspace_protocol::WorkerRole::App(workspace_protocol::SessionId(1)),
                generation: workspace_protocol::ProcessGeneration(1),
            },
            commands,
            jobs: None,
            _rpc: None,
            clipboard_submitted: AtomicU64::new(0),
            clipboard_completed: Mutex::new(ClipboardCompletions::default()),
            shared: Arc::new(Shared {
                snapshot: Mutex::new(Some(SnapshotState {
                    snapshot: Arc::new(snapshot),
                    generation: 1,
                    failed: false,
                })),
                updated: AtomicBool::new(true),
                system_refresh: AtomicBool::new(false),
            }),
            sync: Mutex::new(SyncState::default()),
            completions: Mutex::new(Vec::new()),
        })
    }

    pub(crate) fn connect(connection: &'static WorkerConnection) -> io::Result<Arc<Self>> {
        let (commands, receiver) = tokio::sync::mpsc::channel(64);
        let shared = Arc::new(Shared {
            snapshot: Mutex::new(None),
            updated: AtomicBool::new(false),
            system_refresh: AtomicBool::new(false),
        });
        let (ready, initial) = mpsc::sync_channel(1);
        let process = watchdog::ProcessWatchdog::current()
            .ok_or_else(|| io::Error::other("workspace services require a process watchdog"))?;
        let owner = process
            .register_app(watchdog::AppDescriptor::new(
                watchdog::AppId::from_static("session-services-client"),
                "Tundra shared service operations",
                env!("CARGO_PKG_VERSION"),
                watchdog::AppCriticality::Optional,
            ))
            .map_err(io::Error::other)?;
        let jobs = ServiceJobs::start(&owner)?;
        let rpc = watchdog::OwnedTask::spawn_on(
            crate::process_host::event_loop::runtime().handle(),
            super::client_worker::run(connection, receiver, shared.clone(), ready),
        );
        initial
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| service_error(ServiceError::Unavailable))??;
        Ok(Arc::new(Self {
            worker: connection.worker(),
            commands,
            shared,
            jobs: Some(jobs),
            _rpc: Some(rpc),
            clipboard_submitted: AtomicU64::new(0),
            clipboard_completed: Mutex::new(ClipboardCompletions::default()),
            sync: Mutex::new(SyncState::default()),
            completions: Mutex::new(Vec::new()),
        }))
    }

    pub(crate) fn install(self: &Arc<Self>) {
        if let Ok(paths) = platform::native_platform().app_paths() {
            storage::install_session_config_store(
                storage::StorageLayout::from_app_paths(&paths).config_path,
                self.clone(),
            );
        }
        app::explorer::install_shared_explorer_operations(self.clone());
        let _ = CLIENT.set(self.clone());
    }

    pub(crate) fn current() -> Option<&'static Arc<Self>> {
        CLIENT.get()
    }
    pub(crate) fn worker(&self) -> WorkerId {
        self.worker
    }

    pub(crate) fn snapshot(&self) -> io::Result<(u64, bool, Arc<ServiceSessionSnapshot>)> {
        let state = self
            .shared
            .snapshot
            .lock()
            .map_err(|_| service_error(ServiceError::Unavailable))?;
        let state = state
            .as_ref()
            .ok_or_else(|| service_error(ServiceError::Unavailable))?;
        if JOB_AUTHORIZATION.with(|revision| {
            revision.get().is_some_and(|revision| {
                state
                    .snapshot
                    .authorization
                    .as_ref()
                    .ok()
                    .map(|auth| auth.revision)
                    != Some(revision)
            })
        }) {
            return Err(service_error(ServiceError::Forbidden));
        }
        Ok((state.generation, state.failed, state.snapshot.clone()))
    }

    pub(crate) fn has_updates(&self) -> bool {
        self.shared.updated.load(Ordering::Acquire)
    }

    pub(crate) fn has_pending_writes(&self) -> bool {
        self.completions.try_lock().map_or(true, |pending| {
            pending.iter().any(|pending| pending.is_write())
        })
    }

    pub(crate) fn clipboard_fence(&self) -> u64 {
        self.clipboard_submitted.load(Ordering::Acquire)
    }

    pub(crate) fn clipboard_fence_complete(&self, token: u64) -> bool {
        self.clipboard_completed
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .watermark
            >= token
    }

    pub(crate) fn submit_clipboard_token(&self) -> u64 {
        self.clipboard_submitted.fetch_add(1, Ordering::AcqRel) + 1
    }

    pub(crate) fn complete_clipboard_token(&self, token: u64) {
        let mut completed = self
            .clipboard_completed
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if token <= completed.watermark {
            return;
        }
        completed.pending.insert(token);
        loop {
            let next = completed.watermark + 1;
            if !completed.pending.remove(&next) {
                break;
            }
            completed.watermark = next;
        }
    }

    pub(crate) fn acknowledge_updates(&self) {
        self.shared.updated.store(false, Ordering::Release);
    }

    pub(crate) fn notify_completion(&self) {
        self.shared.updated.store(true, Ordering::Release);
        crate::process_host::event_loop::wake();
    }

    pub(crate) fn take_system_refresh(&self) -> bool {
        self.shared.system_refresh.swap(false, Ordering::AcqRel)
    }

    pub(crate) fn submit(&self, operation: ServiceOperation) -> io::Result<mpsc::Receiver<Reply>> {
        let (reply, receiver) = mpsc::sync_channel(1);
        self.commands
            .try_send(Request { operation, reply })
            .map_err(|_| service_error(ServiceError::Unavailable))?;
        Ok(receiver)
    }

    pub(crate) fn submit_job(&self, job: Box<dyn FnOnce() + Send>) -> io::Result<()> {
        self.jobs
            .as_ref()
            .ok_or_else(|| service_error(ServiceError::Unavailable))?
            .submit(job)
    }

    pub(crate) fn in_authorization<T>(revision: u64, work: impl FnOnce() -> T) -> T {
        JOB_AUTHORIZATION.with(|current| {
            let previous = current.replace(Some(revision));
            let result = work();
            current.set(previous);
            result
        })
    }

    pub(crate) fn authorized(
        operation: ServiceOperation,
        authorization_revision: u64,
    ) -> ServiceOperation {
        ServiceOperation::Authorized {
            authorization_revision,
            operation: Box::new(operation),
        }
    }

    /// Blocking adapter for storage and file-operation workers. UI writes use submit.
    pub(crate) fn request(&self, operation: ServiceOperation) -> io::Result<ServiceValue> {
        self.submit(operation)?
            .recv_timeout(REQUEST_TIMEOUT)
            .map_err(|_| service_error(ServiceError::Unavailable))?
            .map_err(service_error)
    }

    pub(crate) fn decode<T: DeserializeOwned>(domain: &ServiceDomain) -> io::Result<(u64, T)> {
        let version = domain
            .as_ref()
            .map_err(|error| service_error(error.clone()))?;
        serde_json::from_value(version.value.clone())
            .map(|value| (version.revision, value))
            .map_err(io::Error::other)
    }

    pub(crate) fn read<T: DeserializeOwned>(
        &self,
        operation: ServiceOperation,
    ) -> io::Result<(u64, T)> {
        let (_, _, snapshot) = self.snapshot()?;
        let cached = match &operation {
            ServiceOperation::ReadAuthorization => Some(&snapshot.authorization),
            ServiceOperation::ReadConfig => Some(&snapshot.config),
            ServiceOperation::ReadProfile => Some(&snapshot.profile),
            ServiceOperation::ReadClockProfile => Some(&snapshot.clock_profile),
            ServiceOperation::ReadClockTime => Some(&snapshot.clock_time),
            ServiceOperation::ReadFileOperations => Some(&snapshot.files),
            ServiceOperation::ReadSystemSnapshot => Some(&snapshot.system),
            _ => None,
        };
        if let Some(domain) = cached {
            return Self::decode(domain);
        }
        match self.request(operation)? {
            ServiceValue::Versioned { revision, value } => serde_json::from_value(value)
                .map(|value| (revision, value))
                .map_err(io::Error::other),
            ServiceValue::Empty => Err(io::Error::other("missing workspace service value")),
        }
    }

    pub(crate) fn encode(value: &impl Serialize) -> io::Result<serde_json::Value> {
        serde_json::to_value(value).map_err(io::Error::other)
    }

    pub(crate) fn read_config(&self) -> io::Result<(u64, storage::StorageConfig)> {
        let (_, _, snapshot) = self.snapshot()?;
        let (revision, mut config): (_, storage::StorageConfig) = Self::decode(&snapshot.config)?;
        config.session_revision = Some(revision);
        config.session_authorization_revision = Some(
            snapshot
                .authorization
                .as_ref()
                .map_err(|error| service_error(error.clone()))?
                .revision,
        );
        if JOB_AUTHORIZATION.with(|revision| {
            revision
                .get()
                .is_some_and(|revision| Some(revision) != config.session_authorization_revision)
        }) {
            return Err(service_error(ServiceError::Forbidden));
        }
        Ok((revision, config))
    }

    pub(crate) fn request_system_status(
        &self,
        active: Option<bool>,
        refresh: bool,
    ) -> io::Result<()> {
        let (_, _, snapshot) = self.snapshot()?;
        let revision = snapshot
            .authorization
            .as_ref()
            .map_err(|error| service_error(error.clone()))?
            .revision;
        let receiver = self.submit(Self::authorized(
            ServiceOperation::RequestSystemStatus { active, refresh },
            revision,
        ))?;
        self.completions
            .lock()
            .map_err(|_| service_error(ServiceError::Unavailable))?
            .push(crate::session::services_client::PendingWrite::silent(
                receiver,
            ));
        Ok(())
    }
}

pub(crate) fn service_error(error: ServiceError) -> io::Error {
    let message = match error {
        ServiceError::Conflict => i18n::render_current(&i18n::msg!("workspace-services-conflict")),
        ServiceError::Forbidden => {
            i18n::render_current(&i18n::msg!("workspace-services-forbidden"))
        }
        ServiceError::OperationFailed { message } => serde_json::from_value(message)
            .map(|message| i18n::render_current(&message))
            .unwrap_or_else(|_| {
                i18n::render_current(&i18n::msg!("workspace-services-unavailable"))
            }),
        ServiceError::InvalidRequest | ServiceError::Unavailable => {
            i18n::render_current(&i18n::msg!("workspace-services-unavailable"))
        }
    };
    io::Error::other(message)
}

impl storage::SessionConfigStore for SessionServicesClient {
    fn load_config(&self) -> Result<storage::StorageConfig, storage::StorageError> {
        self.read_config()
            .map(|(_, config)| config)
            .map_err(storage_error)
    }

    fn save_config(&self, config: &storage::StorageConfig) -> Result<(), storage::StorageError> {
        let save = || -> io::Result<()> {
            let revision = config
                .session_revision
                .ok_or_else(|| io::Error::other("configuration was not read"))?;
            let auth_revision = config
                .session_authorization_revision
                .ok_or_else(|| io::Error::other("authorization was not read"))?;
            self.request(Self::authorized(
                ServiceOperation::WriteConfig {
                    revision,
                    config: Self::encode(config)?,
                },
                auth_revision,
            ))?;
            Ok(())
        };
        save().map_err(storage_error)
    }
}

fn storage_error(error: io::Error) -> storage::StorageError {
    storage::StorageError::Platform {
        message: error.to_string(),
    }
}

impl ExplorerSharedOperations for SessionServicesClient {
    fn execute(
        &self,
        request: ExplorerSharedRequest,
    ) -> Result<ExplorerSharedSnapshot, ExplorerError> {
        let read = || -> io::Result<ExplorerSharedSnapshot> {
            if matches!(request, ExplorerSharedRequest::Snapshot) {
                return self
                    .read(ServiceOperation::ReadFileOperations)
                    .map(|(_, value)| value);
            }
            let (_, _, snapshot) = self.snapshot()?;
            let revision = snapshot
                .authorization
                .as_ref()
                .map_err(|error| service_error(error.clone()))?
                .revision;
            self.read(Self::authorized(
                ServiceOperation::UpdateFileOperations {
                    operation: Self::encode(&request)?,
                },
                revision,
            ))
            .map(|(_, value)| value)
        };
        read().map_err(|error| ExplorerError::InvalidOperation(error.to_string()))
    }
}
