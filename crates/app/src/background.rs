//! Runs one computation at a time and retains only the latest request and result.
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use watchdog::{ManagedTaskGroup, ManagedThreadHandle, TaskId, TaskSpec};

static NEXT_JOB: AtomicU64 = AtomicU64::new(1);

type Cancel = Arc<AtomicBool>;
type Work<T> = Box<dyn FnOnce(&AtomicBool) -> T + Send>;

struct State<T> {
    pending: Option<(Cancel, Work<T>)>,
    active: Cancel,
    result: Option<T>,
    running: bool,
}

pub struct Latest<T>(Arc<Mutex<State<T>>>, Mutex<Option<ManagedThreadHandle<()>>>);

impl<T> Default for Latest<T> {
    fn default() -> Self {
        Self(
            Arc::new(Mutex::new(State {
                pending: None,
                active: Arc::new(AtomicBool::new(false)),
                result: None,
                running: false,
            })),
            Mutex::new(None),
        )
    }
}
impl<T> std::fmt::Debug for Latest<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Latest(..)")
    }
}
// Cloned page snapshots do not share mutable job ownership.
impl<T> Clone for Latest<T> {
    fn clone(&self) -> Self {
        Self::default()
    }
}
impl<T> PartialEq for Latest<T> {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}
impl<T> Eq for Latest<T> {}
impl<T> Latest<T> {
    pub fn cancel(&self) {
        let mut state = self.0.lock().unwrap();
        state.active.store(true, Ordering::Relaxed);
        state.pending = None;
        state.result = None;
    }
    pub fn take(&self) -> Option<T> {
        self.0.lock().unwrap().result.take()
    }
    pub fn busy(&self) -> bool {
        let state = self.0.lock().unwrap();
        state.running || state.result.is_some()
    }
    pub fn ready(&self) -> bool {
        self.0.lock().unwrap().result.is_some()
    }
}
impl<T: Send + 'static> Latest<T> {
    pub fn submit_in(
        &self,
        group: &ManagedTaskGroup,
        work: impl FnOnce(&AtomicBool) -> T + Send + 'static,
    ) -> Result<(), watchdog::WatchdogError> {
        self.submit_with_group(Some(group), work)
    }

    pub(crate) fn submit_with_group(
        &self,
        group: Option<&ManagedTaskGroup>,
        work: impl FnOnce(&AtomicBool) -> T + Send + 'static,
    ) -> Result<(), watchdog::WatchdogError> {
        let mut state = self.0.lock().unwrap();
        state.active.store(true, Ordering::Relaxed);
        let cancelled = Arc::new(AtomicBool::new(false));
        state.active = cancelled.clone();
        state.pending = Some((cancelled, Box::new(work)));
        state.result = None;
        if state.running {
            return Ok(());
        }
        state.running = true;
        let shared = self.0.clone();
        drop(state);
        let mut finish = Some(Finish {
            state: shared.clone(),
            armed: true,
        });
        let mut run = move |cancellation: Option<watchdog::ThreadCancellation>| {
            let mut finish = finish.take().expect("computation runs once");
            if let Some(cancellation) = &cancellation {
                let weak = Arc::downgrade(&shared);
                cancellation.on_cancel(move || {
                    if let Some(shared) = weak.upgrade() {
                        let mut state = shared.lock().unwrap_or_else(|e| e.into_inner());
                        state.active.store(true, Ordering::Release);
                        state.pending = None;
                        state.result = None;
                    }
                });
            }
            loop {
                if cancellation.as_ref().is_some_and(|c| c.is_cancelled()) {
                    return;
                }
                let job = {
                    let mut state = shared.lock().unwrap();
                    match state.pending.take() {
                        Some(job) => job,
                        None => {
                            state.running = false;
                            finish.armed = false;
                            drop(state);
                            platform::task_events::notify();
                            return;
                        }
                    }
                };
                let (cancelled, work) = job;
                let result = work(&cancelled);
                let mut state = shared.lock().unwrap();
                if !cancelled.load(Ordering::Acquire)
                    && !cancellation.as_ref().is_some_and(|c| c.is_cancelled())
                {
                    state.result = Some(result);
                    drop(state);
                    platform::task_events::notify();
                }
            }
        };
        if let Some(group) = group {
            group.reap_finished();
            let id = TaskId::new(format!(
                "compute-{}",
                NEXT_JOB.fetch_add(1, Ordering::Relaxed)
            ))
            .unwrap();
            let handle = group
                .spawn_thread_with_cancellation(TaskSpec::one_shot(id), move |cancel| {
                    run(Some(cancel))
                })?;
            *self.1.lock().unwrap() = Some(handle);
        } else {
            // Standalone library use has no process runtime. Never start an
            // unowned thread; the application installs watchdog before UI work.
            run(None);
        }
        Ok(())
    }
}

struct Finish<T> {
    state: Arc<Mutex<State<T>>>,
    armed: bool,
}
impl<T> Drop for Finish<T> {
    fn drop(&mut self) {
        if self.armed {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            state.active.store(true, Ordering::Release);
            state.pending = None;
            state.result = None;
            state.running = false;
            drop(state);
            platform::task_events::notify();
        }
    }
}

impl<T> Drop for Latest<T> {
    fn drop(&mut self) {
        self.cancel();
    }
}
