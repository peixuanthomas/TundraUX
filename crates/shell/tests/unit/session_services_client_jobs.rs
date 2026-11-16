use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc as blocking,
};
use std::time::{Duration, Instant};

struct Fixture {
    runtime: Option<watchdog::WatchdogRuntime>,
    owner: AppWatchdog,
    root: std::path::PathBuf,
}

impl Fixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let root = std::env::temp_dir().join(format!(
            "tundra-services-jobs-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let config = watchdog::WatchdogConfig::new(
            root.join("reports"),
            root.join("fallback"),
            root.join("data"),
            "services-test",
            env!("CARGO_PKG_VERSION"),
        );
        let (runtime, process) = watchdog::WatchdogRuntime::start_isolated(config).unwrap();
        let owner = process
            .register_app(watchdog::AppDescriptor::new(
                watchdog::AppId::from_static("service-jobs-test"),
                "Service jobs test",
                "1",
                watchdog::AppCriticality::Optional,
            ))
            .unwrap();
        Self {
            runtime: Some(runtime),
            owner,
            root,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            let _ = runtime.shutdown();
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn until(mut ready: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(2);
    while !ready() {
        assert!(Instant::now() < deadline, "service job worker did not stop");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn dropping_the_last_sender_stops_an_idle_worker_without_cancellation() {
    let fixture = Fixture::new();
    let mut jobs = ServiceJobs::start(&fixture.owner).unwrap();
    let worker = jobs.worker.take().unwrap();
    drop(jobs);
    until(|| worker.is_finished());
    assert_eq!(worker.join().unwrap(), Some(()));
}

#[test]
fn cancelling_an_idle_worker_wakes_it_while_the_owner_keeps_the_sender() {
    let fixture = Fixture::new();
    let jobs = ServiceJobs::start(&fixture.owner).unwrap();
    let worker = jobs.worker.as_ref().unwrap();
    worker.cancel();
    until(|| worker.is_finished() && jobs.sender.is_closed());
}

#[test]
fn dropping_a_busy_owner_does_not_join_or_execute_later_queued_writes() {
    let fixture = Fixture::new();
    let jobs = ServiceJobs::start(&fixture.owner).unwrap();
    let (entered, started) = blocking::sync_channel(1);
    let (release, wait) = blocking::sync_channel(1);
    jobs.submit(Box::new(move || {
        entered.send(()).unwrap();
        let _ = wait.recv_timeout(Duration::from_secs(2));
    }))
    .unwrap();
    started.recv_timeout(Duration::from_secs(2)).unwrap();
    let ran = Arc::new(AtomicBool::new(false));
    for _ in 0..32 {
        let queued = ran.clone();
        let sender = jobs.sender.clone();
        jobs.submit(Box::new(move || {
            drop(sender);
            queued.store(true, Ordering::Release);
        }))
        .unwrap();
    }
    let closed = jobs.sender.clone();
    let started = Instant::now();
    drop(jobs);
    assert!(started.elapsed() < Duration::from_millis(500));
    release.send(()).unwrap();
    until(|| closed.is_closed());
    assert!(!ran.load(Ordering::Acquire));
}
