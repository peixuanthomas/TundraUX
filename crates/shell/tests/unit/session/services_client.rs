use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use workspace_protocol::{ServiceDomain, ServiceSessionSnapshot, ServiceVersion};

fn domain(value: impl serde::Serialize) -> ServiceDomain {
    Ok(ServiceVersion {
        revision: 7,
        value: serde_json::to_value(value).unwrap(),
    })
}

fn snapshot(auth: Option<&AuthSession>) -> ServiceSessionSnapshot {
    ServiceSessionSnapshot {
        authorization: domain(auth),
        config: domain(storage::StorageConfig::default()),
        profile: Err(ServiceError::Forbidden),
        clock_profile: Err(ServiceError::Forbidden),
        clock_time: domain(Option::<crate::session_services::clock::ClockAnchor>::None),
        files: domain(app::explorer::ExplorerSharedSnapshot::default()),
        system: domain(Option::<app::AppSystemStatusSnapshot>::None),
        system_requests: Err(ServiceError::Forbidden),
    }
}

fn state() -> ShellSession {
    ShellSession::new_for_home_mode(ShellLaunchConfig::default(), (120, 40), ShellHomeMode::User)
}

#[test]
fn pending_save_consumes_no_reply_and_remains_in_close_lifecycle() {
    let mut state = state();
    let client = SessionServicesClient::for_test(snapshot(None));
    let (reply, receiver) = std::sync::mpsc::sync_channel(1);
    let called = Arc::new(AtomicUsize::new(0));
    let completed = called.clone();
    let task = state.register_workspace_save();
    client.completions.lock().unwrap().push(PendingWrite {
        receiver,
        session: Some(None),
        page: Some(state.active_page_id()),
        complete: Some(Box::new(move |_, result| {
            assert!(result.is_ok());
            completed.fetch_add(1, Ordering::Relaxed);
        })),
        task: Some(task),
        clipboard: None,
    });
    client.sync_session(&mut state);
    assert!(client.has_pending_writes());
    assert!(state.tasks.has_running_operations());
    assert_eq!(called.load(Ordering::Relaxed), 0);
    reply.send(Ok(ServiceValue::Empty)).unwrap();
    assert!(client.sync_session(&mut state));
    assert!(!client.has_pending_writes());
    assert!(!state.tasks.has_running_operations());
    assert_eq!(called.load(Ordering::Relaxed), 1);
}

#[test]
fn failed_save_cancels_normal_close_and_exposes_the_failure() {
    let mut state = state();
    let client = SessionServicesClient::for_test(snapshot(None));
    let (reply, receiver) = std::sync::mpsc::sync_channel(1);
    let task = state.register_workspace_save();
    state.window_close = super::super::app_host::WindowClose::Confirmed;
    client.completions.lock().unwrap().push(PendingWrite {
        receiver,
        session: Some(None),
        page: Some(state.active_page_id()),
        task: Some(task),
        clipboard: None,
        complete: Some(Box::new(|state, result| {
            state.pages.common.error_message = Some(result.unwrap_err().to_string().into());
        })),
    });
    reply.send(Err(ServiceError::Conflict)).unwrap();
    client.sync_session(&mut state);
    assert_eq!(
        state.window_close,
        super::super::app_host::WindowClose::Cancelled
    );
    assert!(state.pages.common.error_message.is_some());
    assert!(!client.has_pending_writes());
}

#[test]
fn a_closed_page_never_delivers_its_save_to_a_replacement_page() {
    let mut state = state();
    state.enter_new_page(ShellScreen::Clock);
    let page = state.active_page_id();
    let client = SessionServicesClient::for_test(snapshot(None));
    let (reply, receiver) = std::sync::mpsc::sync_channel(1);
    let called = Arc::new(AtomicUsize::new(0));
    let completed = called.clone();
    client.completions.lock().unwrap().push(PendingWrite {
        receiver,
        session: Some(None),
        page: Some(page),
        task: None,
        clipboard: None,
        complete: Some(Box::new(move |_, _| {
            completed.fetch_add(1, Ordering::Relaxed);
        })),
    });
    state.close_page_instance(page);
    state.enter_new_page(ShellScreen::Clock);
    assert_ne!(state.active_page_id(), page);
    reply.send(Ok(ServiceValue::Empty)).unwrap();
    client.sync_session(&mut state);
    assert_eq!(called.load(Ordering::Relaxed), 0);
}

#[test]
fn an_old_session_reply_is_retired_without_touching_the_next_session() {
    let mut state = state();
    let auth = AuthSession {
        source: identity::IdentitySource::LocalAccount,
        session_id: "next".into(),
        user_id: "next".into(),
        username: "next".into(),
        role: UserRole::Guest,
        started_at_epoch_ms: 1,
    };
    state.app.dispatch_at(
        app::AppCommand::SetAuthSession(Some(auth.clone())),
        Instant::now(),
    );
    let client = SessionServicesClient::for_test(snapshot(Some(&auth)));
    let (reply, receiver) = std::sync::mpsc::sync_channel(1);
    let called = Arc::new(AtomicUsize::new(0));
    let completed = called.clone();
    client.completions.lock().unwrap().push(PendingWrite {
        receiver,
        session: Some(Some("old".into())),
        page: Some(state.active_page_id()),
        task: None,
        clipboard: None,
        complete: Some(Box::new(move |_, _| {
            completed.fetch_add(1, Ordering::Relaxed);
        })),
    });
    reply.send(Ok(ServiceValue::Empty)).unwrap();
    client.sync_session(&mut state);
    assert_eq!(called.load(Ordering::Relaxed), 0);
    assert_eq!(state.app.auth_session(), Some(&auth));
}

#[test]
fn unavailable_configuration_is_an_error_instead_of_a_successful_default() {
    let mut snapshot = snapshot(None);
    snapshot.config = Err(ServiceError::Unavailable);
    let client = SessionServicesClient::for_test(snapshot);
    assert!(client.read_config().is_err());
    assert!(storage::SessionConfigStore::load_config(client.as_ref()).is_err());
}

#[test]
fn clipboard_fence_finishes_after_success_or_error_feedback_and_ignores_other_saves() {
    for result in [Ok(ServiceValue::Empty), Err(ServiceError::Conflict)] {
        let mut state = state();
        let client = SessionServicesClient::for_test(snapshot(None));
        let token = client.submit_clipboard_token();
        let (reply, receiver) = std::sync::mpsc::sync_channel(1);
        let callback_client = client.clone();
        client.completions.lock().unwrap().push(PendingWrite {
            receiver,
            session: Some(None),
            page: Some(state.active_page_id()),
            task: None,
            clipboard: Some(token),
            complete: Some(Box::new(move |state, result| {
                assert!(!callback_client.clipboard_fence_complete(token));
                state.pages.common.error_message = Some(if result.is_ok() {
                    "copied".into()
                } else {
                    "failed".into()
                });
            })),
        });
        let (_unrelated_reply, unrelated) = std::sync::mpsc::sync_channel(1);
        let task = state.register_workspace_save();
        client.completions.lock().unwrap().push(PendingWrite {
            receiver: unrelated,
            session: Some(None),
            page: Some(state.active_page_id()),
            task: Some(task),
            clipboard: None,
            complete: None,
        });
        assert_eq!(client.clipboard_fence(), token);
        assert!(!client.clipboard_fence_complete(token));
        let expected = if result.is_ok() { "copied" } else { "failed" };
        reply.send(result).unwrap();
        client.sync_session(&mut state);
        assert!(client.clipboard_fence_complete(token));
        assert!(client.has_pending_writes());
        assert_eq!(
            state
                .pages
                .common
                .error_message
                .as_ref()
                .unwrap()
                .render_current(),
            expected
        );
    }
}

#[test]
fn clipboard_fence_is_retired_when_its_owner_page_or_session_is_gone() {
    for old_session in [false, true] {
        let mut state = state();
        let client = SessionServicesClient::for_test(snapshot(None));
        let token = client.submit_clipboard_token();
        let (reply, receiver) = std::sync::mpsc::sync_channel(1);
        client.completions.lock().unwrap().push(PendingWrite {
            receiver,
            session: Some(if old_session {
                Some("old".into())
            } else {
                None
            }),
            page: Some(PageInstanceId::new()),
            task: None,
            clipboard: Some(token),
            complete: Some(Box::new(|_, _| {
                panic!("retired clipboard feedback reached a different owner")
            })),
        });
        reply.send(Ok(ServiceValue::Empty)).unwrap();
        client.sync_session(&mut state);
        assert!(client.clipboard_fence_complete(token));
    }
}

#[test]
fn a_later_clipboard_reply_cannot_complete_an_earlier_inputs_fence() {
    let client = SessionServicesClient::for_test(snapshot(None));
    let first = client.submit_clipboard_token();
    let second = client.submit_clipboard_token();
    client.complete_clipboard_token(second);
    assert!(!client.clipboard_fence_complete(first));
    assert!(!client.clipboard_fence_complete(second));
    client.complete_clipboard_token(first);
    assert!(client.clipboard_fence_complete(first));
    assert!(client.clipboard_fence_complete(second));
}

#[test]
fn an_unchanged_unavailable_domain_does_not_repeat_feedback_or_redraw() {
    let mut state = state();
    let mut snapshot = snapshot(None);
    snapshot.config = Err(ServiceError::Unavailable);
    let client = SessionServicesClient::for_test(snapshot);
    assert!(client.sync_session(&mut state));
    assert!(!client.sync_session(&mut state));
}

#[test]
fn an_authorization_update_for_the_same_session_keeps_the_window_authenticated() {
    let mut state = state();
    let mut auth = AuthSession {
        source: identity::IdentitySource::LocalAccount,
        session_id: "same".into(),
        user_id: "same".into(),
        username: "same".into(),
        role: UserRole::User,
        started_at_epoch_ms: 1,
    };
    state.app.dispatch_at(
        app::AppCommand::SetAuthSession(Some(auth.clone())),
        Instant::now(),
    );
    auth.role = UserRole::Admin;
    let client = SessionServicesClient::for_test(snapshot(Some(&auth)));
    assert!(client.sync_session(&mut state));
    assert_eq!(state.app.auth_session(), Some(&auth));
}

#[test]
fn a_snapshot_for_another_session_cannot_authenticate_the_current_window() {
    let mut state = state();
    let mut auth = AuthSession {
        source: identity::IdentitySource::LocalAccount,
        session_id: "old".into(),
        user_id: "same".into(),
        username: "same".into(),
        role: UserRole::User,
        started_at_epoch_ms: 1,
    };
    state.app.dispatch_at(
        app::AppCommand::SetAuthSession(Some(auth.clone())),
        Instant::now(),
    );
    auth.session_id = "new".into();
    let client = SessionServicesClient::for_test(snapshot(Some(&auth)));
    assert!(client.sync_session(&mut state));
    assert!(state.app.auth_session().is_none());
}

#[test]
fn the_linux_time_policy_does_not_make_an_unchanged_snapshot_dirty() {
    let mut state = state();
    state.identity_backend = identity::IdentityBackend::Linux;
    let client = SessionServicesClient::for_test(snapshot(None));
    client.sync_session(&mut state);
    assert_eq!(
        state.app.storage_config().time_sync.source,
        storage::TimeSyncSource::OperatingSystem
    );
    assert!(!client.sync_session(&mut state));
}
