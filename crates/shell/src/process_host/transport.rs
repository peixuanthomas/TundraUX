use super::event_loop;
use platform::shell_control::AsyncLocalConnection;
use serde::{Serialize, de::DeserializeOwned};
use std::collections::{BTreeMap, HashSet, VecDeque};
use std::io;
use std::sync::{Arc, Mutex};
use tokio::io::AsyncWriteExt;
use tokio::sync::Notify;
use workspace_protocol::transport::{encode_message, read_payload};
use workspace_protocol::{
    AppToShell, ShellToApp, SupervisorToWorker, WindowFrame, WorkerToSupervisor,
};

const MAX_MESSAGES: usize = 256;
const MAX_CONTROL_BYTES: usize = 8 * 1024 * 1024;

pub trait Message: Serialize + DeserializeOwned + Clone + Send + 'static {
    fn frame(&self) -> Option<&WindowFrame> {
        None
    }
    fn frame_mut(&mut self) -> Option<&mut WindowFrame> {
        None
    }
    fn resident_bytes(&self) -> usize {
        size_of_val(self)
    }
}

fn value_bytes(value: &serde_json::Value) -> usize {
    size_of_val(value)
        + match value {
            serde_json::Value::String(text) => text.capacity(),
            serde_json::Value::Array(values) => values.iter().map(value_bytes).sum(),
            serde_json::Value::Object(values) => values
                .iter()
                .map(|(key, value)| {
                    key.capacity()
                        + size_of::<String>()
                        + 3 * size_of::<usize>()
                        + value_bytes(value)
                })
                .sum(),
            _ => 0,
        }
}

impl Message for ShellToApp {
    fn resident_bytes(&self) -> usize {
        size_of_val(self)
            + match self {
                Self::Input { event, .. } => value_bytes(event),
                Self::Init(init) => {
                    value_bytes(&init.launch)
                        + init.profile.as_ref().map_or(0, value_bytes)
                        + init.language.as_ref().map_or(0, value_bytes)
                }
                Self::TaskAction { id, .. } | Self::OpenLauncherEntry { id } => id.capacity(),
                Self::TaskCatalog { ids } => ids
                    .iter()
                    .map(|id| size_of::<String>() + id.capacity())
                    .sum(),
                _ => 0,
            }
    }
}
impl Message for WorkerToSupervisor {
    fn resident_bytes(&self) -> usize {
        use workspace_protocol::{DesktopOperation, ShellCompletion};
        size_of_val(self)
            + match self {
                Self::SpawnApp { application, .. } => application.capacity(),
                Self::LaunchDesktop { operation, .. } => match operation {
                    DesktopOperation::OpenPath { path } => path.0.as_os_str().len(),
                    DesktopOperation::OpenWith { path, application } => {
                        path.0.as_os_str().len() + application.0.as_os_str().len()
                    }
                    DesktopOperation::OpenUri { uri } => uri.capacity(),
                },
                Self::Finished(ShellCompletion::UpdatePrepared(path)) => path.as_os_str().len(),
                _ => 0,
            }
    }
}
impl Message for SupervisorToWorker {
    fn resident_bytes(&self) -> usize {
        size_of_val(self)
            + match self {
                Self::AppStarted { endpoint, .. } | Self::ServicesStarted { endpoint, .. } => {
                    endpoint.capacity()
                }
                Self::CleanupError { error, .. } => error.capacity(),
                Self::DesktopLaunched { error, .. } => error.as_ref().map_or(0, String::capacity),
                _ => 0,
            }
    }
}
impl Message for AppToShell {
    fn resident_bytes(&self) -> usize {
        if self.frame().is_some() {
            return 0;
        }
        size_of_val(self)
            + match self {
                Self::TaskAction { id, .. } | Self::LaunchFailed(id) => id.capacity(),
                Self::OpenApp(value) => value_bytes(value),
                Self::Status(status) => status
                    .task_ids
                    .iter()
                    .map(|id| size_of::<String>() + id.capacity())
                    .sum(),
                Self::ShellAction(workspace_protocol::ShellAction::LanguageReloaded { code }) => {
                    code.capacity()
                }
                Self::ShellAction(workspace_protocol::ShellAction::UpdatePrepared(path)) => {
                    path.as_os_str().len()
                }
                _ => 0,
            }
    }
    fn frame(&self) -> Option<&WindowFrame> {
        if let Self::Frame(frame) = self {
            Some(frame)
        } else {
            None
        }
    }
    fn frame_mut(&mut self) -> Option<&mut WindowFrame> {
        if let Self::Frame(frame) = self {
            Some(frame)
        } else {
            None
        }
    }
}

struct State<M> {
    messages: VecDeque<M>,
    in_flight: bool,
    error: Option<(io::ErrorKind, String)>,
    bytes: usize,
}

struct Queue<M> {
    state: Mutex<State<M>>,
    available: Notify,
    space: Notify,
    budget: usize,
}

impl<M: Message> Queue<M> {
    fn new(limit: usize) -> Arc<Self> {
        Arc::new(Self {
            state: Mutex::new(State {
                messages: VecDeque::new(),
                in_flight: false,
                error: None,
                bytes: 0,
            }),
            available: Notify::new(),
            space: Notify::new(),
            budget: limit.min(MAX_CONTROL_BYTES),
        })
    }

    fn push(&self, mut message: M) -> Result<(), (io::Error, M)> {
        let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
        if let Some((kind, error)) = &state.error {
            return Err((io::Error::new(*kind, error.clone()), message));
        }
        let bytes = message.resident_bytes();
        if bytes > self.budget {
            return Err((io::ErrorKind::InvalidInput.into(), message));
        }
        if bytes > self.budget.saturating_sub(state.bytes) {
            return Err((io::ErrorKind::WouldBlock.into(), message));
        }
        if let Some(frame) = message.frame_mut() {
            if let Some(index) = state
                .messages
                .iter()
                .position(|message| message.frame().is_some())
            {
                let mut previous = state.messages.remove(index).expect("pending frame");
                if let Some(previous) = previous.frame_mut() {
                    if frame.startup_timings.is_none() {
                        frame.startup_timings = previous.startup_timings.take();
                    }
                }
            }
        } else if state
            .messages
            .iter()
            .filter(|message| message.frame().is_none())
            .count()
            >= MAX_MESSAGES
        {
            return Err((io::ErrorKind::WouldBlock.into(), message));
        }
        state.messages.push_back(message);
        state.bytes += bytes;
        drop(state);
        self.available.notify_one();
        Ok(())
    }

    fn fail(&self, error: &io::Error) {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .error = Some((error.kind(), error.to_string()));
        self.available.notify_one();
        self.space.notify_one();
        event_loop::wake();
    }

    async fn next(&self) -> io::Result<M> {
        loop {
            let available = self.available.notified();
            {
                let mut state = self.state.lock().unwrap_or_else(|error| error.into_inner());
                if let Some(message) = state.messages.pop_front() {
                    state.bytes -= message.resident_bytes();
                    state.in_flight = true;
                    self.space.notify_one();
                    return Ok(message);
                }
                if let Some((kind, error)) = &state.error {
                    return Err(io::Error::new(*kind, error.clone()));
                }
            }
            available.await;
        }
    }

    fn complete(&self) {
        self.state
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .in_flight = false;
        event_loop::wake();
    }
}

/// Each direction has a bounded reliable control queue and one replaceable complete frame.
pub struct Connection<Send: Message, Receive: Message> {
    outgoing: Arc<Queue<Send>>,
    incoming: Arc<Queue<Receive>>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl<Send: Message, Receive: Message> Connection<Send, Receive> {
    pub fn new(stream: AsyncLocalConnection, limit: usize) -> Self {
        let outgoing = Queue::<Send>::new(limit);
        let incoming = Queue::<Receive>::new(limit);
        let (mut reader, mut writer) = stream.split();
        let send = outgoing.clone();
        let receive = incoming.clone();
        let write_task = event_loop::runtime().spawn(async move {
            let result = async {
                let mut images = HashSet::new();
                loop {
                    let message = send.next().await?;
                    let (bytes, next_images) = event_loop::codec(move || {
                        let mut message = message;
                        let next_images = message.frame_mut().map(|frame| {
                            let mut next = HashSet::new();
                            for image in &mut frame.images {
                                if !next.insert(image.image_id) || images.contains(&image.image_id)
                                {
                                    image.rgba = Arc::default();
                                }
                            }
                            next
                        });
                        let bytes = encode_message(&message, limit)?;
                        Ok((bytes, next_images.unwrap_or(images)))
                    })
                    .await?;
                    writer.write_all(&bytes).await?;
                    images = next_images;
                    send.complete();
                }
                #[allow(unreachable_code)]
                Ok::<(), io::Error>(())
            }
            .await;
            if let Err(error) = result {
                send.fail(&error);
                receive.fail(&error);
            }
        });
        let send = outgoing.clone();
        let receive = incoming.clone();
        let read_task = event_loop::runtime().spawn(async move {
            let result = async {
                let mut assets = BTreeMap::<u64, Arc<[u8]>>::new();
                let mut sequence = 0;
                loop {
                    let payload = read_payload(&mut reader, limit).await?;
                    let (mut message, next_assets) = event_loop::codec(move || {
                        let mut message: Receive = serde_json::from_slice(&payload)
                            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
                        if let Some(frame) = message.frame_mut() {
                            frame.validate()?;
                            let mut next = BTreeMap::new();
                            let mut bytes = 0;
                            for image in &mut frame.images {
                                if image.rgba.is_empty() {
                                    image.rgba = next
                                        .get(&image.image_id)
                                        .or_else(|| assets.get(&image.image_id))
                                        .cloned()
                                        .ok_or_else(|| {
                                            io::Error::new(
                                                io::ErrorKind::InvalidData,
                                                "missing frame image",
                                            )
                                        })?;
                                }
                                if image.rgba.len() as u64
                                    != u64::from(image.width) * u64::from(image.height) * 4
                                {
                                    return Err(io::ErrorKind::InvalidData.into());
                                }
                                if !next.contains_key(&image.image_id) {
                                    bytes += image.rgba.len();
                                    if bytes > workspace_protocol::MAX_IMAGE_BYTES {
                                        return Err(io::ErrorKind::InvalidData.into());
                                    }
                                    next.insert(image.image_id, image.rgba.clone());
                                }
                            }
                            assets = next;
                        }
                        Ok((message, assets))
                    })
                    .await?;
                    assets = next_assets;
                    if let Some(frame) = message.frame() {
                        if frame.sequence <= sequence {
                            continue;
                        }
                        sequence = frame.sequence;
                    }
                    loop {
                        let space = receive.space.notified();
                        match receive.push(message) {
                            Ok(()) => break,
                            Err((error, returned)) if error.kind() == io::ErrorKind::WouldBlock => {
                                message = returned;
                                space.await;
                            }
                            Err((error, _)) => return Err(error),
                        }
                    }
                    event_loop::wake();
                }
                #[allow(unreachable_code)]
                Ok::<(), io::Error>(())
            }
            .await;
            if let Err(error) = result {
                send.fail(&error);
                receive.fail(&error);
            }
        });
        Self {
            outgoing,
            incoming,
            tasks: vec![write_task, read_task],
        }
    }

    pub fn queue(&mut self, message: &Send) -> io::Result<()> {
        self.queue_owned(message.clone())
    }

    pub fn queue_owned(&mut self, message: Send) -> io::Result<()> {
        self.outgoing.push(message).map_err(|(error, _)| error)
    }

    pub fn pending_write(&self) -> bool {
        let state = self
            .outgoing
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        state.in_flight || !state.messages.is_empty()
    }

    pub fn poll(&mut self) -> io::Result<Vec<Receive>> {
        let mut state = self
            .incoming
            .state
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if state.messages.is_empty()
            && let Some((kind, error)) = &state.error
        {
            return Err(io::Error::new(*kind, error.clone()));
        }
        let messages = state.messages.drain(..).collect();
        state.bytes = 0;
        drop(state);
        self.incoming.space.notify_one();
        Ok(messages)
    }
}

impl<Send: Message, Receive: Message> Drop for Connection<Send, Receive> {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/process_host/transport_tests.rs"]
mod tests;
