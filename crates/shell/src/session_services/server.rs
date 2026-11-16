use super::state::ServiceState;
use crate::process_host::{WorkerConnection, event_loop};
use platform::shell_control::{AsyncLocalConnection, AsyncLocalListener};
use std::collections::HashMap;
use std::io;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use workspace_protocol::{
    MAX_SERVICE_FRAME_BYTES, PROTOCOL_VERSION, ServiceClientMessage, ServiceHostMessage,
    ServiceReply, SupervisorToWorker, WorkerId, WorkerToSupervisor,
    transport::{encode_message, read_payload},
};

type Allowed = Arc<Mutex<HashMap<WorkerId, u32>>>;

pub(crate) fn run_session_services(
    connection: &'static WorkerConnection,
    _watchdog: watchdog::ProcessWatchdog,
) -> io::Result<()> {
    let platform: Arc<dyn platform::Platform> = Arc::from(platform::native_platform());
    let storage = storage::StorageManager::open_from_platform(platform.as_ref())
        .map_err(io::Error::other)?
        .manager;
    let state = Arc::new(Mutex::new(
        ServiceState::new(storage).map_err(io::Error::other)?,
    ));
    let runtime = event_loop::runtime();
    let listener = {
        let _entered = runtime.enter();
        AsyncLocalListener::bind(
            connection
                .endpoint()
                .ok_or_else(|| io::Error::other("missing workspace services endpoint"))?,
        )?
    };
    let allowed = Allowed::default();
    let (changes, _) = tokio::sync::watch::channel(0u64);
    let accept = watchdog::OwnedTask::spawn_on(
        runtime.handle(),
        accept_peers(
            listener,
            allowed.clone(),
            state.clone(),
            platform,
            connection.worker(),
            changes.clone(),
        ),
    );
    connection.send(WorkerToSupervisor::Ready)?;
    let result = 'service: loop {
        let generation = event_loop::generation();
        while let Some(message) = connection.try_recv()? {
            match message {
                SupervisorToWorker::RegisterWorker { worker, pid } => {
                    state
                        .lock()
                        .map_err(|_| io::Error::other("services state poisoned"))?
                        .register(worker);
                    allowed
                        .lock()
                        .map_err(|_| io::Error::other("services registry poisoned"))?
                        .insert(worker, pid);
                    connection.send(WorkerToSupervisor::Registered { worker })?;
                }
                SupervisorToWorker::UnregisterWorker { worker } => {
                    allowed
                        .lock()
                        .map_err(|_| io::Error::other("services registry poisoned"))?
                        .remove(&worker);
                    state
                        .lock()
                        .map_err(|_| io::Error::other("services state poisoned"))?
                        .unregister(worker);
                    changes.send_modify(|generation| *generation = generation.wrapping_add(1));
                }
                SupervisorToWorker::Close | SupervisorToWorker::Exit => {
                    connection.send(WorkerToSupervisor::Exiting { code: 0 })?;
                    break 'service Ok(());
                }
                _ => {}
            }
        }
        if connection.shutdown_requested() {
            break Ok(());
        }
        if accept.is_finished() {
            break Err(io::Error::other("services listener stopped"));
        }
        connection.progress()?;
        event_loop::wait_since(generation, Duration::from_millis(100));
    };
    allowed
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clear();
    changes.send_modify(|generation| *generation = generation.wrapping_add(1));
    accept.abort();
    result
}

async fn accept_peers(
    listener: AsyncLocalListener,
    allowed: Allowed,
    state: Arc<Mutex<ServiceState>>,
    platform: Arc<dyn platform::Platform>,
    services_worker: WorkerId,
    changes: tokio::sync::watch::Sender<u64>,
) -> io::Result<()> {
    let permits = Arc::new(tokio::sync::Semaphore::new(64));
    let mut peers = tokio::task::JoinSet::new();
    loop {
        while peers.try_join_next().is_some() {}
        let stream = match listener.accept().await {
            Ok(stream) => stream,
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => continue,
            Err(error) => return Err(error),
        };
        let pid = stream.peer_pid()?;
        if !allowed
            .lock()
            .map_err(|_| io::Error::other("services registry poisoned"))?
            .values()
            .any(|allowed| *allowed == pid)
        {
            continue;
        }
        let Ok(permit) = permits.clone().try_acquire_owned() else {
            continue;
        };
        let allowed = allowed.clone();
        let state = state.clone();
        let platform = platform.clone();
        let changes = changes.subscribe();
        peers.spawn_on(
            async move {
                let _permit = permit;
                let _ = serve_peer(
                    stream,
                    pid,
                    allowed,
                    state,
                    platform,
                    services_worker,
                    changes,
                )
                .await;
            },
            &tokio::runtime::Handle::current(),
        );
    }
}

async fn serve_peer(
    connection: AsyncLocalConnection,
    pid: u32,
    allowed: Allowed,
    state: Arc<Mutex<ServiceState>>,
    platform: Arc<dyn platform::Platform>,
    services_worker: WorkerId,
    mut changes: tokio::sync::watch::Receiver<u64>,
) -> io::Result<()> {
    let mut stream = connection.into_inner();
    let bytes = tokio::time::timeout(
        Duration::from_secs(2),
        read_payload(&mut stream, MAX_SERVICE_FRAME_BYTES),
    )
    .await
    .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))??;
    let hello: ServiceClientMessage =
        event_loop::codec(move || serde_json::from_slice(&bytes).map_err(io::Error::other)).await?;
    let ServiceClientMessage::Hello(hello) = hello else {
        return Err(io::ErrorKind::PermissionDenied.into());
    };
    if hello.protocol != PROTOCOL_VERSION
        || hello.pid != pid
        || allowed
            .lock()
            .map_err(|_| io::Error::other("services registry poisoned"))?
            .get(&hello.worker)
            != Some(&pid)
    {
        return Err(io::ErrorKind::PermissionDenied.into());
    }
    let welcome = encode_message(
        &ServiceHostMessage::Welcome {
            worker: services_worker,
        },
        MAX_SERVICE_FRAME_BYTES,
    )?;
    stream.write_all(&welcome).await?;
    let mut last_request = None;
    loop {
        let bytes = {
            let read = tokio::time::timeout(
                Duration::from_secs(10),
                read_payload(&mut stream, MAX_SERVICE_FRAME_BYTES),
            );
            tokio::pin!(read);
            loop {
                if allowed
                    .lock()
                    .map_err(|_| io::Error::other("services registry poisoned"))?
                    .get(&hello.worker)
                    != Some(&pid)
                {
                    return Err(io::ErrorKind::PermissionDenied.into());
                }
                tokio::select! {
                    bytes = &mut read => break bytes.map_err(|_| io::Error::from(io::ErrorKind::TimedOut))??,
                    changed = changes.changed() => { changed.map_err(io::Error::other)?; }
                }
            }
        };
        let allowed = allowed.clone();
        let state = state.clone();
        let platform = platform.clone();
        let (id, bytes) = event_loop::codec(move || {
            let ServiceClientMessage::Request(request) = serde_json::from_slice(&bytes)? else {
                return Err(io::ErrorKind::InvalidData.into());
            };
            if last_request.is_some_and(|last| request.id <= last)
                || allowed
                    .lock()
                    .map_err(|_| io::Error::other("services registry poisoned"))?
                    .get(&hello.worker)
                    != Some(&pid)
            {
                return Err(io::ErrorKind::PermissionDenied.into());
            }
            let result = state
                .lock()
                .map_err(|_| io::Error::other("services state poisoned"))?
                .execute(hello.worker, request.operation, platform.as_ref());
            let bytes = encode_message(
                &ServiceHostMessage::Reply(ServiceReply {
                    id: request.id,
                    result,
                }),
                MAX_SERVICE_FRAME_BYTES,
            )?;
            Ok((request.id, bytes))
        })
        .await?;
        last_request = Some(id);
        stream.write_all(&bytes).await?;
    }
}
