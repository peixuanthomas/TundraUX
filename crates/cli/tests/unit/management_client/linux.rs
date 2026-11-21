use super::*;
use crate::management_command::ManagementCli;
fn request(group: &str, arguments: &[&str]) -> ManagementRequest {
    let ManagementCli::Run(request) = crate::management_command::parse_management(
        group,
        &arguments
            .iter()
            .map(|value| (*value).into())
            .collect::<Vec<_>>(),
    )
    .unwrap() else {
        panic!("expected request");
    };
    request
}
#[test]
fn new_actions_keep_backend_ids_and_default_account() {
    for (kind, verb, expected) in [
        (ManagementKind::Firewall, "allow", "add_rule"),
        (ManagementKind::Firewall, "remove", "remove_rule"),
        (ManagementKind::Ssh, "configure", "ssh_configure"),
        (ManagementKind::ScheduledTasks, "preview", "preview_task"),
        (ManagementKind::ScheduledTasks, "run-now", "run_now"),
        (
            ManagementKind::Backups,
            "preview-restore",
            "preview_restore",
        ),
    ] {
        assert_eq!(backend_action(kind, verb, ""), expected);
    }
    let ssh = ManagementSnapshot {
        rows: vec![ManagementRow {
            id: "alice".into(),
            identity: BTreeMap::from([("uid".into(), "1000".into())]),
            actions: vec![ManagementAction {
                id: "ssh_configure".into(),
                ..Default::default()
            }],
            ..Default::default()
        }],
        ..Default::default()
    };
    let (_, selected) = select_action(
        &ssh,
        &request("ssh", &["configure", "--port", "2222"]),
        "ssh_configure",
    )
    .unwrap();
    assert_eq!(selected.unwrap().id, "alice");
}
#[test]
fn previews_keep_generated_text_in_json_and_text_results() {
    let result = ResultData {
        kind: Some(ManagementKind::ScheduledTasks),
        action: "preview_task".into(),
        output: vec!["[Timer]\nOnCalendar=daily".into()],
        ..Default::default()
    };
    let mut output = Vec::new();
    print_result(&result, true, &mut output).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(json["output"][0], "[Timer]\nOnCalendar=daily");
    output.clear();
    print_result(&result, false, &mut output).unwrap();
    assert!(
        String::from_utf8(output)
            .unwrap()
            .contains("OnCalendar=daily")
    );
}
#[test]
fn explicit_wifi_password_selects_replacement_and_open_networks_reject_it() {
    let ManagementCli::Run(mut request) = crate::management_command::parse_management(
        "network",
        &["wifi-connect", "wifi:test:ap", "--password-fd", "9"].map(String::from),
    )
    .unwrap() else {
        panic!("expected a management request");
    };
    let password = ManagementField {
        id: "password".into(),
        required: true,
        secret: true,
        ..Default::default()
    };
    let mut snapshot = ManagementSnapshot {
        rows: vec![ManagementRow {
            id: "wifi:test:ap".into(),
            identity: [("security".into(), "wpa2".into())].into(),
            actions: vec![
                ManagementAction {
                    id: "wifi-connect".into(),
                    ..Default::default()
                },
                ManagementAction {
                    id: "wifi-connect-password".into(),
                    fields: vec![password.clone()],
                    ..Default::default()
                },
            ],
            ..Default::default()
        }],
        ..Default::default()
    };
    let (selected, _) = select_action(&snapshot, &request, "wifi-connect").unwrap();
    assert_eq!(selected.id, "wifi-connect-password");
    assert!(selected.fields.iter().any(|field| field.secret));
    request.password_fd = None;
    let (selected, _) = select_action(&snapshot, &request, "wifi-connect").unwrap();
    assert_eq!(selected.id, "wifi-connect");
    assert!(selected.fields.is_empty());
    request.password_fd = Some(9);
    snapshot.rows[0].actions.pop();
    snapshot.rows[0].actions[0].fields.push(password);
    let (selected, _) = select_action(&snapshot, &request, "wifi-connect").unwrap();
    assert_eq!(selected.id, "wifi-connect");
    assert!(selected.fields.iter().any(|field| field.secret));
    snapshot.rows[0]
        .identity
        .insert("security".into(), "open".into());
    snapshot.rows[0].actions[0].fields.clear();
    assert!(matches!(
        select_action(&snapshot, &request, "wifi-connect"),
        Err(ManagementError::InvalidInput(message)) if message.contains("remove --password-fd")
    ));
    snapshot.rows[0].identity.clear();
    snapshot.rows[0].actions[0].id = "wifi-hidden".into();
    request.values.insert("ssid".into(), "hidden".into());
    request.values.insert("security".into(), "open".into());
    assert!(matches!(
        select_action(&snapshot, &request, "wifi-connect"),
        Err(ManagementError::InvalidInput(_))
    ));
}
#[test]
fn managed_observer_reports_panic_without_replaying_and_can_reconnect() {
    let root =
        std::env::temp_dir().join(format!("tundra-cli-observer-test-{}", std::process::id()));
    let config = watchdog::WatchdogConfig::new(
        root.join("reports"),
        root.join("fallback"),
        root.join("state"),
        "cli-observer-test",
        env!("CARGO_PKG_VERSION"),
    )
    .with_unclean_exit_tracking(false);
    let (runtime, process) = watchdog::WatchdogRuntime::start_isolated(config).unwrap();
    let group = observer_group(&process).unwrap();
    let attempts = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observer_attempts = attempts.clone();
    let (send, receive) = mpsc::channel();
    let worker = spawn_observer(&group, move || {
        observer_attempts.fetch_add(1, Ordering::SeqCst);
        send.send("operation-id").unwrap();
        panic!("observer disconnected after starting an operation");
    })
    .unwrap();
    assert_eq!(worker.join().unwrap(), None);
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    assert_eq!(receive.recv().unwrap(), "operation-id");
    assert!(receive.recv().is_err());
    let deadline = Instant::now() + Duration::from_secs(2);
    let incident = loop {
        if let Some(incident) = runtime.try_recv_incident() {
            break incident;
        }
        assert!(
            Instant::now() < deadline,
            "watchdog did not report the panic"
        );
        std::thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(
        incident.task_id.as_ref().map(watchdog::TaskId::as_str),
        Some("operation-observer")
    );
    assert!(!incident.recovery.is_recovered());
    // Reconnection starts another reader; it must be possible to reuse
    // the completed task slot without restarting the original command.
    let (send, receive) = mpsc::channel();
    let reconnected = spawn_observer(&group, move || {
        send.send("read existing operation-id").unwrap();
    })
    .unwrap();
    assert_eq!(reconnected.join().unwrap(), Some(()));
    assert_eq!(receive.recv().unwrap(), "read existing operation-id");
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
    runtime.shutdown().unwrap();
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn user_service_reload_keeps_the_original_user_and_does_not_elevate() {
    assert!(!configuration_privileged("reload", Some("user")));
    assert!(configuration_privileged("reload", Some("system")));
    assert!(configuration_privileged("reload", None));
    for action in ["apply", "check", "permissions", "history", "restore"] {
        assert!(configuration_privileged(action, Some("user")), "{action}");
    }
}
#[test]
fn secret_descriptor_keeps_the_next_line_and_original_handle_open() {
    let mut descriptors = [0; 2];
    assert_eq!(
        unsafe { libc::pipe2(descriptors.as_mut_ptr(), libc::O_CLOEXEC) },
        0
    );
    let reader = unsafe { File::from_raw_fd(descriptors[0]) };
    let mut writer = unsafe { File::from_raw_fd(descriptors[1]) };
    writer
        .write_all(b"operation-secret\nsecond-secret\n")
        .unwrap();
    drop(writer);
    let mut output = Vec::new();
    assert_eq!(
        &**secret(Some(reader.as_raw_fd()), true, "Secret", &mut output)
            .unwrap()
            .as_ref()
            .unwrap(),
        "operation-secret"
    );
    assert_eq!(
        &**secret(Some(reader.as_raw_fd()), true, "Secret", &mut output)
            .unwrap()
            .as_ref()
            .unwrap(),
        "second-secret"
    );
    assert!(unsafe { libc::fcntl(reader.as_raw_fd(), libc::F_GETFD) } >= 0);
    assert!(output.is_empty());
}
#[test]
fn noninteractive_secret_request_returns_input_required_without_stdin() {
    assert!(
        secret(None, true, "Secret", &mut Vec::new())
            .unwrap()
            .is_none()
    );
}
#[test]
fn partial_package_results_remain_distinct_from_plain_failure() {
    let mut problem = problem::OperationProblem::from_error(&ManagementError::Failed(
        "native operation failed".into(),
    ));
    assert_eq!(failure_state(Some(&problem)), "failed");
    problem.code = "package_partial".into();
    problem.native_exit_code = Some(100);
    assert_eq!(failure_state(Some(&problem)), "partial");
    assert_eq!(problem.exit_code, 1);
    problem.code = "package_busy".into();
    assert_eq!(failure_state(Some(&problem)), "failed");
    assert_eq!(failure_state(None), "failed");
}
#[test]
fn reconnected_configuration_check_keeps_its_result_and_operation_id() {
    for (check, expected, state) in [
        (ConfigCheck::Passed, 0, "succeeded"),
        (ConfigCheck::Failed("Bad configuration".into()), 1, "failed"),
        (
            ConfigCheck::Unavailable("Tool missing".into()),
            4,
            "unsupported",
        ),
        (ConfigCheck::NotChecked, 4, "unchecked"),
    ] {
        let mut result = ResultData {
            operation_id: Some("test-operation".into()),
            document: Some(ConfigDocument {
                path: "/test.conf".into(),
                content: "candidate".into(),
                version: "version".into(),
                existed: true,
                uid: 0,
                gid: 0,
                mode: 0o600,
                validator: "sshd".into(),
                check,
                backup_id: None,
            }),
            ..Default::default()
        };
        complete_result(
            &mut result,
            false,
            true,
            "Configuration check finished".into(),
        );
        assert_eq!(result.code, expected);
        assert_eq!(result.state, state);
        assert_eq!(result.operation_id.as_deref(), Some("test-operation"));
    }
    let mut missing = ResultData::default();
    complete_result(&mut missing, false, true, "Finished".into());
    assert_eq!(missing.code, 7);
    let mut reading = ResultData::default();
    complete_result(&mut reading, false, false, "Read".into());
    assert_eq!(reading.code, 0);
}
#[test]
fn network_reconnect_uses_current_transaction_state_and_keeps_original_ids() {
    for (state, code, expected) in [
        ("AwaitingConfirmation", 7, "awaiting_confirmation"),
        ("Applying", 7, "running"),
        ("Committed", 0, "succeeded"),
        ("Restored", 1, "rolled_back"),
        ("RestoreFailed", 1, "partial"),
    ] {
        let mut result = ResultData {
            operation_id: Some("original-operation".into()),
            transaction_id: Some("transaction".into()),
            ..Default::default()
        };
        let status = ResultData {
            operation_id: Some("status-query".into()),
            snapshot: Some(ManagementSnapshot {
                rows: vec![ManagementRow {
                    id: "transaction".into(),
                    cells: vec![
                        "transaction".into(),
                        "test-interface".into(),
                        state.into(),
                        "30".into(),
                    ],
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        };
        merge_transaction_result(&mut result, status);
        assert_eq!(result.code, code);
        assert_eq!(result.state, expected);
        assert_eq!(result.operation_id.as_deref(), Some("original-operation"));
        assert_eq!(result.transaction_id.as_deref(), Some("transaction"));
    }
}
#[test]
fn input_and_transaction_query_errors_preserve_reconnect_identifiers() {
    let mut result = ResultData {
        operation_id: Some("original-operation".into()),
        transaction_id: Some("transaction".into()),
        ..Default::default()
    };
    client_error(
        &mut result,
        ManagementError::Failed("Secret descriptor closed".into()),
    );
    assert_eq!(result.code, 7);
    assert_eq!(result.state, "unknown");
    assert_eq!(result.operation_id.as_deref(), Some("original-operation"));
    merge_transaction_result(
        &mut result,
        ResultData {
            code: 3,
            message: "Authorization denied".into(),
            ..Default::default()
        },
    );
    assert_eq!(result.code, 3);
    assert_eq!(result.operation_id.as_deref(), Some("original-operation"));
    assert_eq!(result.transaction_id.as_deref(), Some("transaction"));
}
#[test]
fn recovery_preview_reports_content_attributes_and_previous_absence() {
    let original = ConfigDocument {
        path: "/test.conf".into(),
        content: "Port 22\n".into(),
        version: "version".into(),
        existed: true,
        uid: 1000,
        gid: 1000,
        mode: 0o644,
        validator: "sshd".into(),
        check: ConfigCheck::NotChecked,
        backup_id: None,
    };
    let mut recovered = original.clone();
    recovered.content = "Port 2222\n".into();
    recovered.mode = 0o600;
    let difference = recovery_difference(&original, &recovered);
    assert!(difference.contains("-Port 22"));
    assert!(difference.contains("+Port 2222"));
    assert!(difference.contains("Mode: 0644 -> 0600"));
    recovered.existed = false;
    recovered.content.clear();
    assert!(recovery_difference(&original, &recovered).contains("previous absence"));
    let result = ResultData {
        document: Some(recovered),
        preview_diff: Some(difference),
        ..Default::default()
    };
    let mut output = Vec::new();
    print_result(&result, false, &mut output).unwrap();
    assert!(String::from_utf8(output).unwrap().contains("-Port 22"));
}
