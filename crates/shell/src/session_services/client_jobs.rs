use std::io;
use tokio::sync::mpsc;
use watchdog::{AppWatchdog, ManagedThreadHandle, TaskId, TaskSpec};

type Job = Box<dyn FnOnce() + Send>;

pub(super) struct ServiceJobs {
    sender: mpsc::Sender<Option<Job>>,
    worker: Option<ManagedThreadHandle<()>>,
}

impl ServiceJobs {
    pub(super) fn start(owner: &AppWatchdog) -> io::Result<Self> {
        let (sender, mut receiver) = mpsc::channel::<Option<Job>>(32);
        let wake = sender.downgrade();
        let worker = owner
            .task_group("shared-services-operations")
            .spawn_thread_with_cancellation(
                TaskSpec::one_shot(TaskId::from_static("operations")),
                move |cancellation| {
                    let wake = wake.clone();
                    cancellation.on_cancel(move || {
                        if let Some(sender) = wake.upgrade() {
                            let _ = sender.try_send(None);
                        }
                    });
                    while !cancellation.is_cancelled() {
                        match receiver.blocking_recv() {
                            Some(Some(job)) if !cancellation.is_cancelled() => job(),
                            _ => break,
                        }
                    }
                },
            )
            .map_err(io::Error::other)?;
        Ok(Self {
            sender,
            worker: Some(worker),
        })
    }

    pub(super) fn submit(&self, job: Job) -> io::Result<()> {
        self.sender.try_send(Some(job)).map_err(|_| {
            super::client::service_error(workspace_protocol::ServiceError::Unavailable)
        })
    }
}

impl Drop for ServiceJobs {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            worker.cancel();
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/session_services_client_jobs.rs"]
mod tests;
