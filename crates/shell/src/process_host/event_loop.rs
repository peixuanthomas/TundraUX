use std::sync::{Condvar, Mutex, OnceLock};
use std::task::{Wake, Waker};
use std::time::Duration;

struct EventWake;

impl Wake for EventWake {
    fn wake(self: std::sync::Arc<Self>) {
        wake();
    }
}

pub(crate) fn waker() -> Waker {
    Waker::from(std::sync::Arc::new(EventWake))
}

static WAKE: (Mutex<u64>, Condvar) = (Mutex::new(0), Condvar::new());
static ASYNC_WAKE: OnceLock<tokio::sync::watch::Sender<u64>> = OnceLock::new();

pub(crate) fn generation() -> u64 {
    *WAKE.0.lock().unwrap_or_else(|error| error.into_inner())
}

pub(crate) fn wake() {
    let mut generation = WAKE.0.lock().unwrap_or_else(|error| error.into_inner());
    *generation = generation.wrapping_add(1);
    ASYNC_WAKE
        .get_or_init(|| tokio::sync::watch::channel(0).0)
        .send_replace(*generation);
    WAKE.1.notify_all();
}

pub(crate) async fn wait_async_since(generation: u64) {
    let mut receiver = ASYNC_WAKE
        .get_or_init(|| tokio::sync::watch::channel(0).0)
        .subscribe();
    if *receiver.borrow_and_update() == generation {
        let _ = receiver.changed().await;
    }
}

pub(crate) fn wait_since(generation: u64, timeout: Duration) {
    let current = WAKE.0.lock().unwrap_or_else(|error| error.into_inner());
    drop(
        WAKE.1
            .wait_timeout_while(current, timeout, |current| *current == generation)
            .unwrap_or_else(|error| error.into_inner()),
    );
}

pub(crate) fn codec_concurrency() -> usize {
    std::thread::available_parallelism().map_or(1, |cores| cores.get().min(4))
}

pub(crate) fn runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    RUNTIME.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .max_blocking_threads(codec_concurrency())
            .thread_name("workspace-io")
            .enable_all()
            .build()
            .expect("workspace I/O runtime")
    })
}

pub(crate) async fn codec<T: Send + 'static>(
    work: impl FnOnce() -> std::io::Result<T> + Send + 'static,
) -> std::io::Result<T> {
    static PERMITS: OnceLock<std::sync::Arc<tokio::sync::Semaphore>> = OnceLock::new();
    let permit = PERMITS
        .get_or_init(|| std::sync::Arc::new(tokio::sync::Semaphore::new(codec_concurrency())))
        .clone()
        .acquire_owned()
        .await
        .map_err(std::io::Error::other)?;
    watchdog::OwnedTask::spawn_blocking_on(runtime().handle(), move || {
        let _permit = permit;
        work()
    })
    .await
    .map_err(std::io::Error::other)?
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    #[test]
    fn wake_between_inspection_and_wait_does_not_delay_the_loop() {
        let generation = generation();
        wake();
        let started = Instant::now();
        wait_since(generation, Duration::from_secs(1));
        assert!(started.elapsed() < Duration::from_millis(100));
    }

    #[test]
    fn async_wait_observes_a_wake_before_subscription() {
        let generation = generation();
        wake();
        runtime().block_on(async {
            tokio::time::timeout(Duration::from_millis(100), wait_async_since(generation))
                .await
                .unwrap();
        });
    }
}
