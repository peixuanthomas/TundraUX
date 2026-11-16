use super::*;
use std::io;
use std::time::Duration;
use tokio::sync::oneshot;

struct Dropped(Option<oneshot::Sender<()>>);

impl Drop for Dropped {
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn completed_bootstrap_task_returns_its_result() {
    let (started, ready) = oneshot::channel();
    let task = OwnedTask::spawn_on(&Handle::current(), async move {
        started.send(()).unwrap();
        37
    });
    ready.await.unwrap();
    assert!(task.is_finished());
    assert_eq!(task.await.unwrap(), 37);
    assert_eq!(
        OwnedTask::spawn_blocking_on(&Handle::current(), || 41)
            .await
            .unwrap(),
        41
    );
}

#[tokio::test(flavor = "current_thread")]
async fn early_startup_error_cancels_its_listener_owner() {
    async fn fail_startup(dropped: oneshot::Sender<()>) -> io::Result<()> {
        let (started, ready) = oneshot::channel();
        let _listener = OwnedTask::spawn_on(&Handle::current(), async move {
            let _dropped = Dropped(Some(dropped));
            started.send(()).unwrap();
            std::future::pending::<()>().await;
        });
        ready.await.unwrap();
        Err(io::Error::other("startup failed after binding listener"))
    }
    let (dropped, closed) = oneshot::channel();
    assert!(fail_startup(dropped).await.is_err());
    tokio::time::timeout(Duration::from_secs(1), closed)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn dropping_an_awaiting_owner_does_not_detach_its_task() {
    let (started, ready) = oneshot::channel();
    let (dropped, closed) = oneshot::channel();
    let task = OwnedTask::spawn_on(&Handle::current(), async move {
        let _dropped = Dropped(Some(dropped));
        started.send(()).unwrap();
        std::future::pending::<()>().await;
    });
    ready.await.unwrap();
    let mut waiting = Box::pin(async move { task.await });
    assert!(futures_util::poll!(&mut waiting).is_pending());
    drop(waiting);
    tokio::time::timeout(Duration::from_secs(1), closed)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn closing_a_listener_owner_cancels_its_peer_set() {
    let (started, ready) = oneshot::channel();
    let (dropped, closed) = oneshot::channel();
    let listener = OwnedTask::spawn_on(&Handle::current(), async move {
        let mut peers = tokio::task::JoinSet::new();
        peers.spawn_on(
            async move {
                let _dropped = Dropped(Some(dropped));
                started.send(()).unwrap();
                std::future::pending::<()>().await;
            },
            &Handle::current(),
        );
        std::future::pending::<()>().await;
    });
    ready.await.unwrap();
    listener.abort();
    assert!(listener.await.unwrap_err().is_cancelled());
    tokio::time::timeout(Duration::from_secs(1), closed)
        .await
        .unwrap()
        .unwrap();
}
