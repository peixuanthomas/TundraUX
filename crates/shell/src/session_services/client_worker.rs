use super::client::{REQUEST_TIMEOUT, Reply, Request, Shared, SnapshotState};
use crate::process_host::{WorkerConnection, event_loop};
use std::{
    io,
    sync::{Arc, atomic::Ordering, mpsc},
    time::Duration,
};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use workspace_protocol::{
    MAX_SERVICE_FRAME_BYTES, ServiceClientMessage, ServiceError, ServiceHostMessage,
    ServiceOperation, ServiceRequest, ServiceSessionSnapshot, ServiceValue,
    transport::{encode_message, read_payload},
};

pub(super) async fn run(
    connection: &'static WorkerConnection,
    mut requests: tokio::sync::mpsc::Receiver<Request>,
    shared: Arc<Shared>,
    ready: mpsc::SyncSender<io::Result<()>>,
) {
    let initial = async {
        let mut stream = connection.connect_services_async().await?.into_inner();
        let snapshot = read_snapshot(&mut stream, 1).await?;
        super::client::SessionServicesClient::decode::<storage::StorageConfig>(&snapshot.config)?;
        super::client::SessionServicesClient::decode::<Option<identity::AuthSession>>(
            &snapshot.authorization,
        )?;
        publish(&shared, snapshot);
        Ok::<_, io::Error>(stream)
    }
    .await;
    let mut stream = match initial {
        Ok(stream) => {
            let _ = ready.send(Ok(()));
            Some(stream)
        }
        Err(error) => {
            let _ = ready.send(Err(error));
            return;
        }
    };
    let mut next_id = 1;
    let mut interval = tokio::time::interval(Duration::from_millis(250));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    interval.tick().await;
    loop {
        let request = tokio::select! {
            request = requests.recv() => match request { Some(request) => Some(request), None => return },
            _ = interval.tick() => None,
        };
        if stream.is_none() {
            match connection.connect_services_async().await {
                Ok(connection) => stream = Some(connection.into_inner()),
                Err(_) => {
                    failed(&shared);
                    if let Some(request) = request {
                        let _ = request.reply.send(Err(ServiceError::Unavailable));
                    }
                    continue;
                }
            }
        }
        let wire = stream.as_mut().expect("connected services stream");
        let mut reply = None;
        if let Some(request) = &request {
            next_id += 1;
            match transact(wire, next_id, request.operation.clone()).await {
                Ok(result) => reply = Some(result),
                Err(_) => {
                    failed(&shared);
                    let _ = request.reply.send(Err(ServiceError::Unavailable));
                    stream = None;
                    continue;
                }
            }
        }
        next_id += 1;
        match read_snapshot(wire, next_id).await {
            Ok(snapshot) => publish(&shared, snapshot),
            Err(_) => {
                failed(&shared);
                stream = None;
            }
        }
        if let (Some(request), Some(reply)) = (request, reply) {
            let _ = request.reply.send(reply);
            signal(&shared);
        }
    }
}

async fn read_snapshot<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    id: u64,
) -> io::Result<ServiceSessionSnapshot> {
    match transact(stream, id, ServiceOperation::ReadSessionSnapshot)
        .await?
        .map_err(super::client::service_error)?
    {
        ServiceValue::Versioned { value, .. } => {
            event_loop::codec(move || serde_json::from_value(value).map_err(io::Error::other)).await
        }
        ServiceValue::Empty => Err(io::ErrorKind::InvalidData.into()),
    }
}

async fn transact<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    id: u64,
    operation: ServiceOperation,
) -> io::Result<Reply> {
    tokio::time::timeout(REQUEST_TIMEOUT, async {
        let bytes = event_loop::codec(move || {
            encode_message(
                &ServiceClientMessage::Request(ServiceRequest { id, operation }),
                MAX_SERVICE_FRAME_BYTES,
            )
        })
        .await?;
        stream.write_all(&bytes).await?;
        let bytes = read_payload(stream, MAX_SERVICE_FRAME_BYTES).await?;
        let message: ServiceHostMessage =
            event_loop::codec(move || serde_json::from_slice(&bytes).map_err(io::Error::other))
                .await?;
        match message {
            ServiceHostMessage::Reply(reply) if reply.id == id => Ok(reply.result),
            _ => Err(io::ErrorKind::InvalidData.into()),
        }
    })
    .await
    .map_err(|_| io::Error::from(io::ErrorKind::TimedOut))?
}

fn publish(shared: &Shared, snapshot: ServiceSessionSnapshot) {
    let mut current = shared
        .snapshot
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    if current.as_ref().is_some_and(|current| {
        current
            .snapshot
            .authorization
            .as_ref()
            .ok()
            .map(|auth| auth.revision)
            != snapshot
                .authorization
                .as_ref()
                .ok()
                .map(|auth| auth.revision)
    }) {
        shared.system_refresh.store(false, Ordering::Release);
    }
    if snapshot.system_requests.as_ref().is_ok_and(|requests| {
        requests
            .value
            .get("refresh")
            .and_then(serde_json::Value::as_bool)
            == Some(true)
    }) {
        shared.system_refresh.store(true, Ordering::Release);
    }
    if let Some(current) = current.as_mut() {
        let changed = current.failed || *current.snapshot != snapshot;
        current.failed = false;
        if !changed {
            return;
        }
        current.generation += 1;
        current.snapshot = Arc::new(snapshot);
    } else {
        *current = Some(SnapshotState {
            snapshot: Arc::new(snapshot),
            generation: 1,
            failed: false,
        });
    }
    drop(current);
    signal(shared);
}

fn failed(shared: &Shared) {
    if let Ok(mut current) = shared.snapshot.lock()
        && let Some(current) = current.as_mut()
        && !current.failed
    {
        current.failed = true;
        current.generation += 1;
        signal(shared);
    }
}

fn signal(shared: &Shared) {
    shared.updated.store(true, Ordering::Release);
    event_loop::wake();
}

#[cfg(test)]
mod tests {
    use super::*;
    use workspace_protocol::ServiceVersion;

    fn snapshot(authorization_revision: u64, refresh: bool) -> ServiceSessionSnapshot {
        let unavailable = Err(ServiceError::Unavailable);
        ServiceSessionSnapshot {
            authorization: Ok(ServiceVersion {
                revision: authorization_revision,
                value: serde_json::Value::Null,
            }),
            config: unavailable.clone(),
            profile: unavailable.clone(),
            clock_profile: unavailable.clone(),
            clock_time: unavailable.clone(),
            files: unavailable.clone(),
            system: unavailable,
            system_requests: Ok(ServiceVersion {
                revision: 0,
                value: serde_json::json!({"active": false, "refresh": refresh}),
            }),
        }
    }

    #[test]
    fn refresh_survives_a_newer_snapshot_until_consumed_but_never_crosses_login() {
        let shared = Shared {
            snapshot: std::sync::Mutex::new(None),
            updated: std::sync::atomic::AtomicBool::new(false),
            system_refresh: std::sync::atomic::AtomicBool::new(false),
        };
        publish(&shared, snapshot(1, true));
        publish(&shared, snapshot(1, false));
        assert!(shared.system_refresh.swap(false, Ordering::AcqRel));
        publish(&shared, snapshot(1, true));
        publish(&shared, snapshot(2, false));
        assert!(!shared.system_refresh.load(Ordering::Acquire));
    }
}
