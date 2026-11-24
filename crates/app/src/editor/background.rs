pub(super) use crate::background::Latest;
use std::sync::atomic::AtomicBool;
#[cfg(test)]
use std::sync::{Arc, Mutex, atomic::Ordering};
use watchdog::AppWatchdog;

pub const SYNC_BYTES: usize = 64 * 1024;

impl<T: Send + 'static> Latest<T> {
    pub(super) fn submit(&self, work: impl FnOnce(&AtomicBool) -> T + Send + 'static) {
        if let Ok(app) = editor_watchdog() {
            let group = app.map(|app| app.task_group("editor-computations"));
            let _ = self.submit_with_group(group.as_ref(), work);
        }
    }
}

fn editor_watchdog() -> Result<Option<AppWatchdog>, watchdog::WatchdogError> {
    if let Some(app) = AppWatchdog::current() {
        return Ok(Some(app));
    }
    let descriptor = || {
        watchdog::AppDescriptor::new(
            watchdog::AppId::from_static("editor"),
            "Editor",
            env!("CARGO_PKG_VERSION"),
            watchdog::AppCriticality::Optional,
        )
    };
    if let Some(process) = watchdog::ProcessWatchdog::current() {
        return process.register_app(descriptor()).map(Some);
    }
    #[cfg(test)]
    {
        static TEST_RUNTIME: std::sync::OnceLock<(Mutex<watchdog::WatchdogRuntime>, AppWatchdog)> =
            std::sync::OnceLock::new();
        let (_, app) = TEST_RUNTIME.get_or_init(|| {
            let root =
                std::env::temp_dir().join(format!("tundra-editor-tests-{}", std::process::id()));
            let config = watchdog::WatchdogConfig::new(
                root.join("reports"),
                root.join("fallback"),
                root.join("state"),
                "editor-tests",
                env!("CARGO_PKG_VERSION"),
            );
            let (runtime, process) = watchdog::WatchdogRuntime::start_isolated(config).unwrap();
            (
                Mutex::new(runtime),
                process.register_app(descriptor()).unwrap(),
            )
        });
        return Ok(Some(app.clone()));
    }
    #[cfg(not(test))]
    Ok(None)
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::mpsc,
        time::{Duration, Instant},
    };

    #[test]
    fn panic_clears_busy_state_and_allows_another_request() {
        let worker = Latest::<usize>::default();
        worker.submit(|_| panic!("editor computation fixture"));
        let deadline = Instant::now() + Duration::from_secs(3);
        while worker.busy() {
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        worker.submit(|_| 42);
        while !worker.ready() {
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        }
        assert_eq!(worker.take(), Some(42));
    }

    #[test]
    fn process_shutdown_cancels_editor_work_without_dropping_the_editor() {
        let root =
            std::env::temp_dir().join(format!("tundra-editor-cancel-{}", std::process::id()));
        let config = watchdog::WatchdogConfig::new(
            root.join("reports"),
            root.join("fallback"),
            root.join("state"),
            "editor-cancel",
            "test",
        );
        let (runtime, process) = watchdog::WatchdogRuntime::start_isolated(config).unwrap();
        let app = process
            .register_app(watchdog::AppDescriptor::new(
                watchdog::AppId::from_static("editor-test"),
                "Editor",
                "test",
                watchdog::AppCriticality::Optional,
            ))
            .unwrap();
        let worker = Latest::default();
        let (entered, ready) = mpsc::channel();
        let (exited, finished) = mpsc::channel();
        assert!(
            app.run_boundary(
                watchdog::BoundarySpec::new("editor-submit", watchdog::BoundaryKind::UiSession),
                std::panic::AssertUnwindSafe(|| {
                    worker.submit(move |cancel| {
                        entered.send(()).unwrap();
                        while !cancel.load(Ordering::Acquire) {
                            std::thread::sleep(Duration::from_millis(1));
                        }
                        exited.send(()).unwrap();
                        1
                    });
                })
            )
            .is_ok()
        );
        ready.recv_timeout(Duration::from_secs(2)).unwrap();
        runtime.shutdown().unwrap();
        finished.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(!worker.busy());
        assert_eq!(worker.take(), None);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn only_latest_queued_request_runs_and_stale_completion_is_discarded() {
        let worker = Latest::default();
        let (entered, started) = mpsc::channel();
        let (release, blocked) = mpsc::channel();
        worker.submit(move |_| {
            entered.send(()).unwrap();
            blocked.recv().unwrap();
            1
        });
        started.recv_timeout(Duration::from_secs(2)).unwrap();
        let obsolete_ran = Arc::new(AtomicBool::new(false));
        let obsolete = obsolete_ran.clone();
        worker.submit(move |_| {
            obsolete.store(true, Ordering::Relaxed);
            2
        });
        worker.submit(|_| 3);
        release.send(()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        let result = loop {
            if let Some(result) = worker.take() {
                break result;
            }
            assert!(Instant::now() < deadline);
            std::thread::yield_now();
        };
        assert_eq!(result, 3);
        assert!(!obsolete_ran.load(Ordering::Relaxed));
    }

    #[test]
    fn dropping_owner_cancels_running_work() {
        let worker = Latest::default();
        let (entered, started) = mpsc::channel();
        let (exited, finished) = mpsc::channel();
        worker.submit(move |cancel| {
            entered.send(()).unwrap();
            while !cancel.load(Ordering::Relaxed) {
                std::thread::yield_now();
            }
            exited.send(()).unwrap();
        });
        started.recv_timeout(Duration::from_secs(2)).unwrap();
        drop(worker);
        finished.recv_timeout(Duration::from_secs(2)).unwrap();
    }
}
