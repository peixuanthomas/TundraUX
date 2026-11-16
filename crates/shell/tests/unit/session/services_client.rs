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
        input_effect: None,
        automation_authorization: None,
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
        input_effect: None,
        automation_authorization: None,
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

fn consume_workspace_job(
    state: &mut ShellSession,
    client: &Arc<SessionServicesClient>,
    work: impl FnOnce() -> io::Result<()>,
) {
    let outcome = Arc::new(std::sync::Mutex::new(None));
    let result = workspace_job_reply(work(), |result| result.is_ok(), &outcome);
    let (reply, receiver) = std::sync::mpsc::sync_channel(1);
    let task = state.register_workspace_save();
    client.completions.lock().unwrap().push(PendingWrite {
        receiver,
        session: Some(None),
        page: Some(state.active_page_id()),
        task: Some(task),
        input_effect: None,
        automation_authorization: None,
        complete: Some(Box::new(move |state, _| {
            let result = outcome.lock().unwrap().take().unwrap();
            state.pages.common.error_message = result.err().map(|error| error.to_string().into());
        })),
    });
    reply.send(result).unwrap();
    assert!(client.sync_session(state));
    assert!(!client.has_pending_writes());
}

#[test]
fn failed_generic_work_cancels_a_normal_close_and_keeps_its_actual_error() {
    use super::super::app_host::WindowClose;
    for close in [WindowClose::Checking, WindowClose::Confirmed] {
        let mut state = state();
        let client = SessionServicesClient::for_test(snapshot(None));
        assert!(state.begin_window_close_request());
        state.window_close = close;
        consume_workspace_job(&mut state, &client, || {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "write refused",
            ))
        });
        assert_eq!(state.window_close, WindowClose::Cancelled);
        assert_eq!(
            state
                .pages
                .common
                .error_message
                .as_ref()
                .unwrap()
                .render_current(),
            "write refused"
        );
    }
}

#[test]
fn successful_generic_work_allows_a_normal_close_to_finish() {
    use super::super::app_host::WindowClose;
    let mut state = state();
    let client = SessionServicesClient::for_test(snapshot(None));
    assert!(state.begin_window_close_request());
    state.window_close = WindowClose::Confirmed;
    consume_workspace_job(&mut state, &client, || Ok(()));
    assert_eq!(state.window_close, WindowClose::Confirmed);
    assert!(state.pages.common.error_message.is_none());
}

#[test]
fn explicit_close_survives_failure_then_cancel_allows_a_later_normal_close_to_cancel() {
    use super::super::app_host::WindowClose;
    let mut state = state();
    let client = SessionServicesClient::for_test(snapshot(None));
    assert!(state.begin_window_close_request());
    let command = |state: &mut ShellSession, command| {
        state.apply_routed_event_once(
            RoutedEvent {
                input: InputEvent::key(InputKey::Enter),
                target: RoutedTarget::Global,
                command,
            },
            &platform::mock::UnsupportedPlatform,
            Instant::now(),
        );
    };
    command(&mut state, ShellCommand::ConfirmWindowClose);
    assert!(state.window_close_explicit);
    consume_workspace_job(&mut state, &client, || {
        Err(io::Error::other("write refused"))
    });
    assert_eq!(state.window_close, WindowClose::Confirmed);
    assert!(state.pages.common.error_message.is_some());
    command(&mut state, ShellCommand::CancelWindowClose);
    assert_eq!(state.window_close, WindowClose::Cancelled);
    assert!(!state.window_close_explicit);
    state.window_close = WindowClose::Idle;
    assert!(state.begin_window_close_request());
    consume_workspace_job(&mut state, &client, || {
        Err(io::Error::other("second write refused"))
    });
    assert_eq!(state.window_close, WindowClose::Cancelled);
}

#[test]
fn a_new_normal_close_request_clears_a_previous_explicit_choice() {
    use super::super::app_host::WindowClose;
    let mut state = state();
    let client = SessionServicesClient::for_test(snapshot(None));
    state.window_close_explicit = true;
    assert!(state.begin_window_close_request());
    assert!(!state.window_close_explicit);
    consume_workspace_job(&mut state, &client, || {
        Err(io::Error::other("write refused"))
    });
    assert_eq!(state.window_close, WindowClose::Cancelled);
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
        input_effect: None,
        automation_authorization: None,
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
        input_effect: None,
        automation_authorization: None,
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
fn input_effect_fence_finishes_after_success_or_error_feedback_and_ignores_other_saves() {
    for result in [Ok(ServiceValue::Empty), Err(ServiceError::Conflict)] {
        let mut state = state();
        let client = SessionServicesClient::for_test(snapshot(None));
        let token = client.submit_input_effect_token();
        let (reply, receiver) = std::sync::mpsc::sync_channel(1);
        let callback_client = client.clone();
        client.completions.lock().unwrap().push(PendingWrite {
            receiver,
            session: Some(None),
            page: Some(state.active_page_id()),
            task: None,
            input_effect: Some(token),
            automation_authorization: None,
            complete: Some(Box::new(move |state, result| {
                assert!(!callback_client.input_effect_fence_complete(token));
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
            input_effect: None,
            automation_authorization: None,
            complete: None,
        });
        assert_eq!(client.input_effect_fence(), token);
        assert!(!client.input_effect_fence_complete(token));
        let expected = if result.is_ok() { "copied" } else { "failed" };
        reply.send(result).unwrap();
        client.sync_session(&mut state);
        assert!(client.input_effect_fence_complete(token));
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
fn input_effect_fence_is_retired_when_its_owner_page_or_session_is_gone() {
    for old_session in [false, true] {
        let mut state = state();
        let client = SessionServicesClient::for_test(snapshot(None));
        let token = client.submit_input_effect_token();
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
            input_effect: Some(token),
            automation_authorization: None,
            complete: Some(Box::new(|_, _| {
                panic!("retired clipboard feedback reached a different owner")
            })),
        });
        reply.send(Ok(ServiceValue::Empty)).unwrap();
        client.sync_session(&mut state);
        assert!(client.input_effect_fence_complete(token));
    }
}

#[test]
fn a_later_clipboard_reply_cannot_complete_an_earlier_inputs_fence() {
    let client = SessionServicesClient::for_test(snapshot(None));
    let first = client.submit_input_effect_token();
    let second = client.submit_input_effect_token();
    client.complete_input_effect_token(second);
    assert!(!client.input_effect_fence_complete(first));
    assert!(!client.input_effect_fence_complete(second));
    client.complete_input_effect_token(first);
    assert!(client.input_effect_fence_complete(first));
    assert!(client.input_effect_fence_complete(second));
}

#[test]
fn persisted_automation_survives_a_closed_page_before_its_input_effect_retires() {
    let mut state = state();
    let mut cached = snapshot(None);
    cached.config = Err(ServiceError::Unavailable);
    let client = SessionServicesClient::for_test(cached);
    let token = client.submit_input_effect_token();
    let (reply, receiver) = std::sync::mpsc::sync_channel(1);
    client.completions.lock().unwrap().push(PendingWrite {
        receiver,
        session: Some(None),
        page: Some(PageInstanceId::new()),
        complete: Some(Box::new(|_, _| panic!("closed page received UI feedback"))),
        task: None,
        input_effect: Some(token),
        automation_authorization: Some(7),
    });
    client.sync_session(&mut state);
    assert!(!client.input_effect_fence_complete(token));
    assert!(state.automation_commit.is_none());
    reply
        .send(Ok(ServiceValue::Versioned {
            revision: 8,
            value: serde_json::json!({"automation": {"enabled": false}}),
        }))
        .unwrap();
    client.sync_session(&mut state);
    assert_eq!(
        state
            .automation_commit
            .as_ref()
            .map(|commit| (commit.enabled, commit.revision)),
        Some((false, 8))
    );
    assert!(client.input_effect_fence_complete(token));
}

#[test]
fn failed_or_disconnected_automation_retires_without_a_false_persisted_commit() {
    for disconnected in [false, true] {
        let mut state = state();
        let client = SessionServicesClient::for_test(snapshot(None));
        let token = client.submit_input_effect_token();
        let (reply, receiver) = std::sync::mpsc::sync_channel(1);
        let feedback = Arc::new(AtomicUsize::new(0));
        let delivered = feedback.clone();
        let callback_client = client.clone();
        client.completions.lock().unwrap().push(PendingWrite {
            receiver,
            session: Some(None),
            page: Some(state.active_page_id()),
            complete: Some(Box::new(move |_, result| {
                assert!(result.is_err());
                assert!(!callback_client.input_effect_fence_complete(token));
                delivered.fetch_add(1, Ordering::Relaxed);
            })),
            task: None,
            input_effect: Some(token),
            automation_authorization: Some(7),
        });
        if !disconnected {
            reply.send(Err(ServiceError::Unavailable)).unwrap();
        }
        drop(reply);
        client.sync_session(&mut state);
        assert_eq!(feedback.load(Ordering::Relaxed), 1);
        assert!(client.input_effect_fence_complete(token));
        assert!(state.automation_commit.is_none());
    }
}

#[test]
fn authorization_revision_change_discards_old_automation_but_retires_its_effect() {
    let mut state = state();
    let client = SessionServicesClient::for_test(snapshot(None));
    let token = client.submit_input_effect_token();
    let (reply, receiver) = std::sync::mpsc::sync_channel(1);
    client.completions.lock().unwrap().push(PendingWrite {
        receiver,
        session: Some(None),
        page: None,
        complete: None,
        task: None,
        input_effect: Some(token),
        automation_authorization: Some(6),
    });
    reply
        .send(Ok(ServiceValue::Versioned {
            revision: 8,
            value: serde_json::json!({"automation": {"enabled": false}}),
        }))
        .unwrap();
    client.sync_session(&mut state);
    assert!(state.automation_commit.is_none());
    assert!(client.input_effect_fence_complete(token));
}

#[test]
fn locally_changed_login_rejects_an_automation_commit_despite_a_stale_config_revision() {
    let mut state = state();
    let auth = AuthSession {
        source: identity::IdentitySource::LocalAccount,
        session_id: "new-session".into(),
        user_id: "user".into(),
        username: "user".into(),
        role: UserRole::Guest,
        started_at_epoch_ms: 1,
    };
    state
        .app
        .dispatch_at(app::AppCommand::SetAuthSession(Some(auth)), Instant::now());
    state.record_automation_commit(workspace_protocol::AutomationCommit {
        enabled: false,
        revision: 8,
        authorization_revision: 7,
        authorization_session: Some("old-session".into()),
    });
    assert!(state.automation_commit.is_none());
}

#[test]
fn system_defaults_wait_for_the_real_automation_receipt_at_the_common_config_boundary() {
    let mut state = state();
    let mut original = storage::StorageConfig::default();
    original.automation.enabled = true;
    let mut cached = snapshot(None);
    cached.config = domain(&original);
    let mut defaults = original.clone();
    defaults.automation = storage::StorageConfig::default().automation;
    assert!(automation_configuration_changed(&defaults, &cached.config).unwrap());
    let mut unrelated = original.clone();
    unrelated.screen_timeout_seconds = 30;
    assert!(!automation_configuration_changed(&unrelated, &cached.config).unwrap());

    let client = SessionServicesClient::for_test(cached);
    let token = client.submit_input_effect_token();
    let (reply, receiver) = std::sync::mpsc::sync_channel(1);
    client.completions.lock().unwrap().push(PendingWrite {
        receiver,
        session: Some(None),
        page: Some(state.active_page_id()),
        complete: None,
        task: None,
        input_effect: Some(token),
        automation_authorization: Some(7),
    });
    client.sync_session(&mut state);
    assert!(!client.input_effect_fence_complete(token));
    assert!(state.automation_commit.is_none());
    reply
        .send(Ok(ServiceValue::Versioned {
            revision: 8,
            value: serde_json::to_value(defaults).unwrap(),
        }))
        .unwrap();
    client.sync_session(&mut state);
    assert!(client.input_effect_fence_complete(token));
    assert_eq!(
        state
            .automation_commit
            .as_ref()
            .map(|commit| (commit.enabled, commit.revision)),
        Some((false, 8))
    );
}

#[test]
fn a_config_candidate_cannot_be_reauthorized_by_a_newer_submission_snapshot() {
    for changes_automation in [false, true] {
        let mut state = state();
        let initial = snapshot(None);
        let (candidate_authorization, _): (u64, Option<AuthSession>) =
            SessionServicesClient::decode(&initial.authorization).unwrap();
        let mut published_after_validation = initial;
        published_after_validation
            .authorization
            .as_mut()
            .unwrap()
            .revision = 8;
        let client = SessionServicesClient::for_test(published_after_validation);
        let result = state.submit_workspace_write_kind(
            client.as_ref(),
            ServiceOperation::WriteConfig {
                revision: 7,
                config: serde_json::to_value(storage::StorageConfig::default()).unwrap(),
            },
            true,
            changes_automation,
            Some(candidate_authorization),
            |_, _| panic!("unsubmitted configuration received a completion"),
        );
        assert_eq!(
            result.unwrap_err().to_string(),
            service_error(ServiceError::Forbidden).to_string()
        );
        assert!(client.completions.lock().unwrap().is_empty());
        assert_eq!(client.input_effect_fence(), 0);
        assert!(state.automation_commit.is_none());
    }
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
