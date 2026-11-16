//! Result delivery can wake a host loop without depending on its UI runtime.
use std::sync::{OnceLock, mpsc};

static NOTIFIER: OnceLock<fn()> = OnceLock::new();

/// Installs the current process's event-loop notifier. Processes without a host loop omit it.
pub fn install_notifier(notifier: fn()) {
    let _ = NOTIFIER.set(notifier);
}

/// Call after publishing shared state, once its locks have been released.
pub fn notify() {
    if let Some(notifier) = NOTIFIER.get() {
        notifier();
    }
}

#[derive(Debug)]
pub struct Sender<T> {
    sender: mpsc::Sender<T>,
    notifier: fn(),
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            notifier: self.notifier,
        }
    }
}

impl<T> Sender<T> {
    pub fn send(&self, event: T) -> Result<(), mpsc::SendError<T>> {
        self.sender.send(event)?;
        (self.notifier)();
        Ok(())
    }

    /// Retains an already observed event without waking the consumer again.
    pub fn requeue(&self, event: T) -> Result<(), mpsc::SendError<T>> {
        self.sender.send(event)
    }
}

pub fn channel<T>() -> (Sender<T>, mpsc::Receiver<T>) {
    let (sender, receiver) = mpsc::channel();
    (
        Sender {
            sender,
            notifier: notify,
        },
        receiver,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn successful_publications_wake_once_and_preserve_order() {
        static NOTIFICATIONS: AtomicUsize = AtomicUsize::new(0);
        fn notified() {
            NOTIFICATIONS.fetch_add(1, Ordering::SeqCst);
        }
        let (sender, receiver) = mpsc::channel();
        let sender = Sender {
            sender,
            notifier: notified,
        };
        sender.send(1).unwrap();
        sender.clone().send(2).unwrap();
        sender.requeue(3).unwrap();
        assert_eq!(NOTIFICATIONS.load(Ordering::SeqCst), 2);
        assert_eq!(receiver.try_iter().collect::<Vec<_>>(), [1, 2, 3]);
        drop(receiver);
        assert_eq!(sender.send(4).unwrap_err().0, 4);
        assert_eq!(NOTIFICATIONS.load(Ordering::SeqCst), 2);
    }
}
