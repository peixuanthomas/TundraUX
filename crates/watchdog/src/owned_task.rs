use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::runtime::Handle;
use tokio::task::{JoinError, JoinHandle};

/// Owns bootstrap or low-level I/O work that may run before an AppWatchdog exists.
#[must_use = "dropping the owner cancels its task"]
pub struct OwnedTask<T> {
    join: JoinHandle<T>,
}

impl<T: Send + 'static> OwnedTask<T> {
    pub fn spawn_on(runtime: &Handle, future: impl Future<Output = T> + Send + 'static) -> Self {
        Self {
            join: runtime.spawn(future),
        }
    }

    /// Callers must bound admission; already running blocking work cannot be interrupted.
    pub fn spawn_blocking_on(
        runtime: &Handle,
        operation: impl FnOnce() -> T + Send + 'static,
    ) -> Self {
        Self {
            join: runtime.spawn_blocking(operation),
        }
    }
}

impl<T> OwnedTask<T> {
    pub fn abort(&self) {
        self.join.abort();
    }
    pub fn is_finished(&self) -> bool {
        self.join.is_finished()
    }
}

impl<T> Future for OwnedTask<T> {
    type Output = Result<T, JoinError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.get_mut().join).poll(cx)
    }
}

impl<T> Drop for OwnedTask<T> {
    fn drop(&mut self) {
        self.join.abort();
    }
}

#[cfg(test)]
#[path = "../tests/unit/owned_task.rs"]
mod tests;
