use std::cell::Cell;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::thread::{self, ThreadId};

thread_local! {
    static LINE_SCANS: Cell<usize> = const { Cell::new(0) };
}

pub(super) fn record_line_scan() {
    LINE_SCANS.set(LINE_SCANS.get() + 1);
}

pub(super) fn take_line_scans() -> usize {
    LINE_SCANS.replace(0)
}

fn counts() -> &'static Mutex<HashMap<ThreadId, usize>> {
    static COUNTS: OnceLock<Mutex<HashMap<ThreadId, usize>>> = OnceLock::new();
    COUNTS.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(super) fn increment() {
    let mut counts = counts().lock().expect("Markdown parse count lock poisoned");
    let count = counts.entry(thread::current().id()).or_default();
    *count = count.saturating_add(1);
}

pub(super) fn reset() {
    counts()
        .lock()
        .expect("Markdown parse count lock poisoned")
        .insert(thread::current().id(), 0);
}

pub(super) fn get() -> usize {
    counts()
        .lock()
        .expect("Markdown parse count lock poisoned")
        .get(&thread::current().id())
        .copied()
        .unwrap_or(0)
}
