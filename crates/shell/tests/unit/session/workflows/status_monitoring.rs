use super::*;

struct SystemStatusServiceGuard(Option<system_services::SystemServicesHandle>);

impl Drop for SystemStatusServiceGuard {
    fn drop(&mut self) {
        if let Some(handle) = self.0.take() {
            let _ = handle.shutdown();
        }
    }
}

struct SystemStatusTestWeatherProvider;

impl system_services::WeatherProvider for SystemStatusTestWeatherProvider {
    fn current_weather<'life0, 'async_trait>(
        &'life0 self,
        _location: system_services::WeatherLocation,
        _units: system_services::WeatherUnits,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<system_services::WeatherData, String>>
                + Send
                + 'async_trait,
        >,
    >
    where
        'life0: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async { Err("weather disabled in system status test".into()) })
    }
}

#[test]
fn system_status_history_deduplicates_uptime_rejects_stale_and_caps_queues() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut state, UserRole::Admin);
    let first = system_status_metric_snapshot(10);
    state.apply_system_status_snapshot(first.clone());
    state.apply_system_status_snapshot(first);
    assert_eq!(state.system_status_history.cpu.len(), 1);
    state.apply_system_status_snapshot(system_status_metric_snapshot(11));
    assert_eq!(state.system_status_history.cpu.len(), 2);
    let mut stale = system_status_metric_snapshot(12);
    stale.metrics.uptime = system_services::MetricState::Stale {
        last_good: system_services::UptimeSnapshot { seconds: 12 },
        error: "old".into(),
    };
    state.apply_system_status_snapshot(stale);
    assert_eq!(
        state.system_status_history.cpu.len(),
        2,
        "stale uptime inserts no fake points"
    );
    for uptime in 20..90 {
        state.apply_system_status_snapshot(system_status_metric_snapshot(uptime));
    }
    for len in [
        state.system_status_history.cpu.len(),
        state.system_status_history.memory.len(),
        state.system_status_history.network_received.len(),
        state.system_status_history.network_transmitted.len(),
        state.system_status_history.temperature.len(),
    ] {
        assert!(len <= 60);
    }
}

#[test]
fn system_status_history_uses_rolling_observation_window() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut state, UserRole::Admin);
    let base = Utc::now();
    for (uptime, seconds) in [(1, 0), (2, 30), (3, 60)] {
        let mut sample = system_status_metric_snapshot(uptime);
        sample.metrics.sampled_at = base + chrono::Duration::seconds(seconds);
        state.apply_system_status_snapshot(sample);
    }
    assert_eq!(
        state.system_status_history.cpu.len(),
        3,
        "the exactly-60-second boundary remains"
    );
    let mut sample = system_status_metric_snapshot(4);
    sample.metrics.sampled_at = base + chrono::Duration::seconds(61);
    state.apply_system_status_snapshot(sample);
    assert_eq!(state.system_status_history.cpu.len(), 3);
    assert_eq!(
        state.system_status_history.cpu.front().unwrap().observed_at,
        base + chrono::Duration::seconds(30)
    );
    let cpu = state
        .to_system_status_view_model()
        .unwrap()
        .dashboard
        .wide_widgets
        .into_iter()
        .find(|w| w.kind == ui::SystemStatusWidgetKind::Cpu)
        .unwrap();
    assert_eq!(
        cpu.trend.unwrap().len(),
        3,
        "presentation exposes only retained chronological values"
    );
}

#[test]
fn system_status_history_expires_without_new_ready_insertions() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut state, UserRole::Admin);
    let base = Utc::now();
    let mut ready = system_status_metric_snapshot(1);
    ready.metrics.sampled_at = base;
    state.apply_system_status_snapshot(ready);
    let mut later = system_status_metric_snapshot(2);
    later.metrics.sampled_at = base + chrono::Duration::seconds(61);
    later.metrics.uptime = system_services::MetricState::Loading;
    later.metrics.cpu = system_services::MetricState::Stale {
        last_good: system_services::CpuSnapshot {
            usage_percent: 25.0,
            per_core_percent: vec![25.0],
            logical_core_count: 1,
            physical_core_count: Some(1),
        },
        error: "stale".into(),
    };
    later.metrics.memory = system_services::MetricState::Unavailable {
        reason: "unavailable".into(),
    };
    later.metrics.network_io = system_services::MetricState::Loading;
    later.metrics.thermal = system_services::MetricState::Stale {
        last_good: vec![],
        error: "stale".into(),
    };
    state.apply_system_status_snapshot(later);
    assert!(state.system_status_history.cpu.is_empty());
    assert!(state.system_status_history.memory.is_empty());
    assert!(state.system_status_history.network_received.is_empty());
    assert!(state.system_status_history.network_transmitted.is_empty());
    assert!(state.system_status_history.temperature.is_empty());
    let model = state.to_system_status_view_model().unwrap();
    for kind in [
        ui::SystemStatusWidgetKind::Cpu,
        ui::SystemStatusWidgetKind::Memory,
        ui::SystemStatusWidgetKind::Network,
    ] {
        let trend = model
            .dashboard
            .wide_widgets
            .iter()
            .find(|widget| widget.kind == kind)
            .unwrap()
            .trend
            .as_ref()
            .unwrap();
        assert!(
            trend.is_empty(),
            "expired {kind:?} points are not presented"
        );
    }
}

#[test]
fn system_status_pending_refresh_is_inert_for_keyboard_mouse_and_direct_call() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut state, UserRole::Admin);
    state.enter_screen(ShellScreen::SystemStatus);
    state.pages.common.focused_component = ShellComponent::SystemStatus;
    state
        .pages
        .system_status
        .system_status_refresh_requested_revision = Some(41);
    state.pages.system_status.system_status_dashboard_feedback = Some("keep feedback".into());
    let marker = state
        .pages
        .system_status
        .system_status_refresh_requested_revision;
    let feedback = state
        .pages
        .system_status
        .system_status_dashboard_feedback
        .clone();
    let model = state.to_system_status_view_model().unwrap();
    assert!(model.dashboard.actions.refresh_disabled);
    assert_eq!(
        state.route_key_input(&KeyInput::from_label("r")).1,
        ShellCommand::Noop
    );
    let ui::ShellLayout::Full { main, .. } = ui::compute_shell_layout(Rect::new(0, 0, 120, 40))
    else {
        panic!()
    };
    let refresh = ui::system_status_layout(main, &model).refresh_button;
    for kind in [
        ui::MouseEventKind::Down(PointerButton::Left),
        ui::MouseEventKind::Click(PointerButton::Left),
    ] {
        state.apply_input(InputEvent::Mouse(ui::MouseEvent::new(
            refresh.x, refresh.y, kind,
        )));
    }
    state.refresh_system_status();
    assert_eq!(
        state
            .pages
            .system_status
            .system_status_refresh_requested_revision,
        marker
    );
    assert_eq!(
        state.pages.system_status.system_status_dashboard_feedback,
        feedback
    );
}

#[test]
fn system_status_role_gate_and_missing_service_reject_open() {
    for role in [UserRole::Admin, UserRole::User, UserRole::Guest] {
        let mut state = ShellSession::new_for_home_mode(
            ShellLaunchConfig::default(),
            (120, 40),
            ShellHomeMode::User,
        );
        set_test_auth_role(&mut state, role);
        let labels = state
            .user_home_entries()
            .into_iter()
            .map(|entry| entry.label)
            .collect::<Vec<_>>();
        assert_eq!(
            labels.contains(&"System Status".to_string()),
            role != UserRole::Guest
        );
        assert!(!labels.contains(&"Diagnostics".to_string()));
        let original_focus = state.pages.common.focused_component;
        state.open_system_status();
        assert_eq!(state.active_screen(), ShellScreen::Home);
        assert_eq!(state.pages.common.focused_component, original_focus);
        if role != UserRole::Guest {
            assert_eq!(state.status(), "System Status service unavailable");
        }
    }
}

#[test]
fn diagnostics_is_integrated_into_system_status_tabs() {
    for role in [UserRole::Admin, UserRole::User] {
        let mut state = ShellSession::new_for_home_mode(
            ShellLaunchConfig::default(),
            (120, 40),
            ShellHomeMode::User,
        );
        set_test_auth_role(&mut state, role);

        state.apply_input(InputEvent::from_key_label("D"));
        assert_eq!(state.active_screen(), ShellScreen::Home);

        state.apply_routed_event_once(
            RoutedEvent {
                input: InputEvent::from_key_label("D"),
                target: RoutedTarget::Global,
                command: ShellCommand::OpenDiagnostics,
            },
            &platform::mock::UnsupportedPlatform,
            Instant::now(),
        );
        assert_eq!(state.active_screen(), ShellScreen::Home);
        assert_eq!(state.status(), "Open Diagnostics from System Status");

        state.enter_screen(ShellScreen::SystemStatus);
        state.pages.common.focused_component = ShellComponent::SystemStatus;
        state.open_diagnostics();
        assert_eq!(state.active_screen(), ShellScreen::SystemStatus);
        assert_eq!(
            state.pages.system_status.system_status_tab,
            ui::SystemStatusTab::Health
        );
        assert_eq!(
            state.pages.system_status.system_status_route,
            ui::SystemStatusRoute::Detail(ui::SystemStatusDetail::Diagnostics)
        );
        assert_eq!(state.focused_component(), ShellComponent::SystemStatus);

        state.apply_input(InputEvent::from_key_label("Esc"));
        assert_eq!(state.active_screen(), ShellScreen::SystemStatus);
        assert_eq!(
            state.pages.system_status.system_status_route,
            ui::SystemStatusRoute::Dashboard
        );
        state.apply_input(InputEvent::from_key_label("Esc"));
        assert_eq!(state.active_screen(), ShellScreen::SystemStatus);
        assert!(state.back_action().is_none());
    }
}

#[test]
fn system_status_process_sort_handles_clicks_keys_duplicates_units_and_refresh() {
    use ui::SystemStatusProcessSortColumn::{Cpu, Memory};
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut state, UserRole::User);
    state.app.dispatch_at(
        app::AppCommand::SetActiveSystemStatusDashboard(Some(
            storage::SystemStatusDashboardConfig::for_role("Admin"),
        )),
        Instant::now(),
    );
    state.enter_screen(ShellScreen::SystemStatus);
    state.pages.common.focused_component = ShellComponent::SystemStatus;
    state.open_system_status_detail(storage::SystemStatusWidgetKind::TopProcesses);
    let process = |pid, cpu_percent, memory_bytes| system_services::ProcessMetricSnapshot {
        pid,
        name: format!("worker-{pid}"),
        cpu_percent,
        memory_bytes,
    };
    let a = process(1, 9.0, 2 * 1024 * 1024 * 1024);
    let b = process(2, 80.0, 900 * 1024 * 1024);
    let c = process(3, 9.0, 50 * 1024 * 1024);
    let d = process(4, 20.0, 1024 * 1024 * 1024);
    let mut snapshot = system_status_metric_snapshot(1);
    snapshot.metrics.processes =
        system_services::MetricState::Ready(system_services::ProcessRankingsSnapshot {
            top_cpu: vec![b.clone(), a.clone(), c.clone()],
            top_memory: vec![a, d, b, c],
        });
    state.apply_system_status_snapshot(snapshot.clone());
    let pids = |state: &ShellSession| {
        state
            .to_system_status_view_model()
            .unwrap()
            .detail_widget(ui::SystemStatusDetail::Processes)
            .unwrap()
            .compact_rows
            .iter()
            .map(|row| row[0].parse::<u32>().unwrap())
            .collect::<Vec<_>>()
    };
    assert_eq!(pids(&state), vec![2, 4, 1, 3]);
    let ui::ShellLayout::Full { main, .. } = ui::compute_shell_layout(Rect::new(0, 0, 120, 40))
    else {
        panic!("full layout required");
    };
    for (column, expected, descending) in [
        (Cpu, vec![1, 3, 4, 2], false),
        (Cpu, vec![2, 4, 1, 3], true),
        (Memory, vec![1, 4, 2, 3], true),
        (Memory, vec![3, 2, 4, 1], false),
    ] {
        state.pages.system_status.system_status_selected_row = 2;
        let selected_pid = pids(&state)[2];
        state.pages.system_status.system_status_scroll_offset = 2;
        let layout = ui::system_status_layout(main, &state.to_system_status_view_model().unwrap());
        let area = layout
            .process_sort_headers
            .iter()
            .find(|(col, _)| *col == column)
            .unwrap()
            .1;
        let at = Instant::now();
        state.apply_input_at(
            InputEvent::mouse_down(PointerButton::Left, (area.x, area.y)),
            at,
        );
        state.apply_input_at(
            InputEvent::mouse_up(PointerButton::Left, (area.x, area.y)),
            at + Duration::from_millis(20),
        );
        assert_eq!(pids(&state), expected);
        assert_eq!(
            state
                .pages
                .system_status
                .system_status_process_sort
                .descending,
            descending
        );
        assert_eq!(
            pids(&state)[state.pages.system_status.system_status_selected_row],
            selected_pid
        );
        assert_eq!(state.pages.system_status.system_status_scroll_offset, 0);
    }
    snapshot.revision += 1;
    state.apply_system_status_snapshot(snapshot);
    assert_eq!(pids(&state), vec![3, 2, 4, 1]);
    state.apply_input(InputEvent::key(InputKey::Char('m')));
    assert_eq!(pids(&state), vec![1, 4, 2, 3]);
    state.apply_input(InputEvent::key(InputKey::Char('c')));
    assert_eq!(pids(&state), vec![2, 4, 1, 3]);
}

#[test]
fn system_status_widget_double_click_from_terminal_events_opens_detail() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut state, UserRole::User);
    state.enter_screen(ShellScreen::SystemStatus);
    state.pages.common.focused_component = ShellComponent::SystemStatus;
    let model = state.to_system_status_view_model().unwrap();
    let ui::ShellLayout::Full { main, .. } = ui::compute_shell_layout(Rect::new(0, 0, 120, 40))
    else {
        panic!("system status mouse test requires a full layout");
    };
    let widget = ui::system_status_layout(main, &model)
        .widgets
        .into_iter()
        .find(|widget| widget.kind == ui::SystemStatusWidgetKind::Diagnostics)
        .expect("diagnostics widget");

    let coordinates = (
        widget.area.x.saturating_add(1),
        widget.area.y.saturating_add(1),
    );
    let first_click_at = Instant::now();
    state.apply_input_at(
        InputEvent::mouse_down(PointerButton::Left, coordinates),
        first_click_at,
    );
    state.apply_input_at(
        InputEvent::mouse_up(PointerButton::Left, coordinates),
        first_click_at + Duration::from_millis(50),
    );
    state.apply_input_at(
        InputEvent::mouse_down(PointerButton::Left, coordinates),
        first_click_at + Duration::from_millis(100),
    );

    assert_eq!(state.active_screen(), ShellScreen::SystemStatus);
    assert_eq!(
        state.pages.system_status.system_status_tab,
        ui::SystemStatusTab::Health
    );
    assert_eq!(
        state.pages.system_status.system_status_route,
        ui::SystemStatusRoute::Detail(ui::SystemStatusDetail::Diagnostics)
    );
    assert_eq!(state.focused_component(), ShellComponent::SystemStatus);
}

#[test]
fn system_status_keyboard_navigates_dashboard_and_routes_detail_actions() {
    let mut user = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut user, UserRole::User);
    user.enter_screen(ShellScreen::SystemStatus);
    user.pages.common.focused_component = ShellComponent::SystemStatus;
    assert_eq!(
        user.route_key_input(&KeyInput::from_label("Tab")).1,
        ShellCommand::SystemStatusFocusNext
    );
    assert_eq!(
        user.route_key_input(&KeyInput::from_label("Enter")).1,
        ShellCommand::SystemStatusActivateFocus
    );
    assert_eq!(
        user.route_key_input(&KeyInput::from_label("e")).1,
        ShellCommand::SystemStatusBeginEdit
    );
    user.set_system_status_tab(ui::SystemStatusTab::Health);
    assert_eq!(
        user.route_key_input(&KeyInput::from_label("r")).1,
        ShellCommand::DiagnosticsRescan
    );
    assert_eq!(
        user.route_key_input(&KeyInput::from_label("Down")).1,
        ShellCommand::DiagnosticsNext
    );

    let mut admin = user;
    set_test_auth_role(&mut admin, UserRole::Admin);
    admin.set_system_status_tab(ui::SystemStatusTab::Overview);
    assert_eq!(
        admin.route_key_input(&KeyInput::from_label("Shift+Tab")).1,
        ShellCommand::SystemStatusFocusPrevious
    );
}

#[test]
fn system_status_modules_open_directly_from_dashboard_by_keyboard() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut state, UserRole::Admin);
    state.enter_screen(ShellScreen::SystemStatus);
    state.pages.common.focused_component = ShellComponent::SystemStatus;
    for key in ['l', 'i'] {
        state.apply_input(InputEvent::key(InputKey::Char(key)));
        assert_eq!(
            state.pages.system_status.system_status_route,
            ui::SystemStatusRoute::Dashboard
        );
        assert_eq!(state.active_screen(), ShellScreen::SystemStatus);
    }
    state.apply_input(InputEvent::key(InputKey::Char('h')));
    assert_eq!(
        state.pages.system_status.system_status_route,
        ui::SystemStatusRoute::Detail(ui::SystemStatusDetail::Diagnostics)
    );
    assert_eq!(
        state.pages.diagnostics.diagnostics_tab,
        ui::DiagnosticsTab::Health
    );
}

#[test]
fn system_status_users_share_admin_view_permissions() {
    let persisted = storage::SystemStatusDashboardConfig::for_role("Admin");
    let mut user = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut user, UserRole::User);
    user.app.dispatch_at(
        app::AppCommand::SetActiveSystemStatusDashboard(Some(persisted.clone())),
        Instant::now(),
    );
    assert!(user.system_status_widget_allowed(storage::SystemStatusWidgetKind::TopProcesses));
    assert!(
        user.system_status_widget_detail_allowed(storage::SystemStatusWidgetKind::TopProcesses)
    );
    for (kind, detail) in [
        (
            storage::SystemStatusWidgetKind::Storage,
            ui::SystemStatusDetail::Storage,
        ),
        (
            storage::SystemStatusWidgetKind::Network,
            ui::SystemStatusDetail::Network,
        ),
        (
            storage::SystemStatusWidgetKind::TopProcesses,
            ui::SystemStatusDetail::Processes,
        ),
    ] {
        user.back_from_system_status_detail();
        user.open_system_status_detail(kind);
        assert_eq!(
            user.pages.system_status.system_status_route,
            ui::SystemStatusRoute::Detail(detail)
        );
    }
    assert!(
        user.app
            .active_system_status_dashboard()
            .unwrap()
            .widgets
            .contains(&storage::SystemStatusWidgetKind::TopProcesses)
    );

    user.set_system_status_tab(ui::SystemStatusTab::Storage);
    assert_eq!(
        user.pages.system_status.system_status_tab,
        ui::SystemStatusTab::Storage
    );
    user.set_system_status_tab(ui::SystemStatusTab::Network);
    assert_eq!(
        user.pages.system_status.system_status_tab,
        ui::SystemStatusTab::Network
    );
}

#[test]
fn diagnostics_rejects_guest_even_with_system_status_parent() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut state, UserRole::Guest);
    state.enter_screen(ShellScreen::SystemStatus);
    state.pages.common.focused_component = ShellComponent::SystemStatus;

    state.open_diagnostics();

    assert_eq!(state.active_screen(), ShellScreen::SystemStatus);
    assert_eq!(state.status(), "Open Diagnostics from System Status");
}

#[test]
fn system_status_users_receive_full_storage_and_network_details() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut state, UserRole::Admin);
    let snapshot = system_status_test_snapshot(
        1,
        system_services::StoragePressure::Low,
        true,
        system_services::SystemVolumeSource::FixedVolumeFallback,
    );
    state.apply_system_status_snapshot(snapshot.clone());
    let admin = state.to_system_status_view_model().unwrap();
    let ui::SystemStatusContentViewModel::Admin(admin) = admin.content else {
        panic!()
    };
    assert!(
        admin
            .overview
            .system_volume_usage
            .contains("fixed-volume fallback; source unknown")
    );
    assert_eq!(admin.overview.system_volume_used_percentage, Some(90));
    assert_eq!(admin.overview.network_status, "Connected");
    assert_eq!(
        admin.storage_rows[0].system_volume,
        "Fallback (source unknown)"
    );

    set_test_auth_role(&mut state, UserRole::User);
    let mut stale = snapshot;
    let storage = match stale.storage {
        system_services::StorageState::Ready(value) => value,
        _ => unreachable!(),
    };
    let network = match stale.network {
        system_services::NetworkState::Ready(value) => value,
        _ => unreachable!(),
    };
    stale.storage = system_services::StorageState::Stale {
        last_good: storage,
        error: "/sensitive/mount secret-label".into(),
    };
    stale.network = system_services::NetworkState::Stale {
        last_good: network,
        error: "secret-iface secret-display 192.0.2.99 2001:db8::99".into(),
    };
    state.apply_system_status_snapshot(stale);
    let model = state.to_system_status_view_model().unwrap();
    assert!(matches!(
        model.content,
        ui::SystemStatusContentViewModel::Admin(_)
    ));
    let debug = format!("{model:?}");
    assert!(debug.contains("(stale)"));
    for secret in [
        "/sensitive/mount",
        "secret-label",
        "secret-iface",
        "secret-display",
        "192.0.2.99",
        "2001:db8::99",
    ] {
        assert!(debug.contains(secret), "missing {secret}");
    }
    let mut unavailable = system_status_test_snapshot(
        2,
        system_services::StoragePressure::Normal,
        true,
        system_services::SystemVolumeSource::Detected,
    );
    unavailable.storage = system_services::StorageState::Unavailable {
        reason: "/sensitive/mount".into(),
    };
    unavailable.network = system_services::NetworkState::Unavailable {
        reason: "secret-iface".into(),
    };
    state.apply_system_status_snapshot(unavailable);
    let debug = format!("{:?}", state.to_system_status_view_model().unwrap());
    assert!(debug.contains("Unavailable"));
    assert!(debug.contains("sensitive"));
    assert!(debug.contains("secret-iface"));
}

#[test]
fn system_status_alert_dedupe_upgrade_recovery_and_network_baseline() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut state, UserRole::Admin);
    state.apply_system_status_snapshot(system_status_test_snapshot(
        1,
        system_services::StoragePressure::Low,
        true,
        system_services::SystemVolumeSource::Detected,
    ));
    assert_eq!(
        state.system_status_storage_alerts.get("/sensitive/mount"),
        Some(&SystemStatusAlertLevel::Low)
    );
    assert!(
        state
            .app
            .notification_center()
            .alert()
            .unwrap()
            .render_current()
            .contains("/sensitive/mount")
    );
    assert!(
        state
            .app
            .notification_center()
            .alert()
            .unwrap()
            .render_current()
            .contains("100 B")
    );
    assert_eq!(
        state.app.notification_center().alert_tone(),
        Some(ui::NotificationTone::Warning)
    );
    assert_eq!(state.app.notification_center().alert_count(), 1);
    assert_eq!(
        state.app.notification_center().alert_tone(),
        Some(ui::NotificationTone::Warning)
    );
    assert!(
        state
            .app
            .notification_center()
            .alert_message_for_key("system-status.storage:/sensitive/mount")
            .is_some()
    );
    let repeated = state.app.notification_center().alert().cloned();
    state.apply_system_status_snapshot(system_status_test_snapshot(
        2,
        system_services::StoragePressure::Low,
        true,
        system_services::SystemVolumeSource::Detected,
    ));
    assert_eq!(state.app.notification_center().alert(), repeated.as_ref());
    assert_eq!(state.app.notification_center().alert_count(), 1);
    assert_eq!(
        state.system_status_storage_alerts.get("/sensitive/mount"),
        Some(&SystemStatusAlertLevel::Low)
    );
    state.apply_system_status_snapshot(system_status_test_snapshot(
        3,
        system_services::StoragePressure::Critical,
        false,
        system_services::SystemVolumeSource::Detected,
    ));
    assert_eq!(
        state.system_status_storage_alerts.get("/sensitive/mount"),
        Some(&SystemStatusAlertLevel::Critical)
    );
    assert_eq!(
        state.app.notification_center().alert_tone(),
        Some(ui::NotificationTone::Critical)
    );
    assert_eq!(state.app.notification_center().alert_count(), 2);
    assert!(state.system_status_disconnected_notified);
    state.apply_system_status_snapshot(system_status_test_snapshot(
        4,
        system_services::StoragePressure::Normal,
        true,
        system_services::SystemVolumeSource::Detected,
    ));
    assert!(state.system_status_storage_alerts.is_empty());
    assert!(!state.system_status_disconnected_notified);
    assert!(state.app.notification_center().alert().is_none());
    assert_eq!(state.app.notification_center().alert_count(), 0);
    state.apply_system_status_snapshot(system_status_test_snapshot(
        5,
        system_services::StoragePressure::Low,
        false,
        system_services::SystemVolumeSource::Detected,
    ));
    assert_eq!(
        state.system_status_storage_alerts.get("/sensitive/mount"),
        Some(&SystemStatusAlertLevel::Low)
    );
    assert!(state.system_status_disconnected_notified);
    assert_eq!(state.app.notification_center().alert_count(), 2);
    assert_eq!(
        state.app.notification_center().alert_tone(),
        Some(ui::NotificationTone::Warning)
    );
    state.app.dispatch_at(
        app::AppCommand::SetSystemStatusSnapshot(None),
        Instant::now(),
    );
    state.open_home_for_session(
        AuthSession {
            source: identity::IdentitySource::SystemCurrentProcess,
            session_id: "next-admin-session".into(),
            user_id: "next-admin".into(),
            username: "next-admin".into(),
            role: UserRole::Admin,
            started_at_epoch_ms: 2,
        },
        storage::UserProfile::default(),
    );
    assert!(state.system_status_network_baseline.is_none());
    assert!(state.system_status_storage_alerts.is_empty());
    assert_eq!(state.app.notification_center().alert_count(), 0);

    state.open_home_for_session(
        AuthSession {
            source: identity::IdentitySource::SystemCurrentProcess,
            session_id: "user-session".into(),
            user_id: "user-id".into(),
            username: "user".into(),
            role: UserRole::User,
            started_at_epoch_ms: 3,
        },
        storage::UserProfile::default(),
    );
    state.apply_system_status_snapshot(system_status_test_snapshot(
        10,
        system_services::StoragePressure::Low,
        true,
        system_services::SystemVolumeSource::Detected,
    ));
    let storage_key = "system-status.storage:/sensitive/mount";
    let storage_message = state
        .app
        .notification_center()
        .alert_message_for_key(storage_key)
        .unwrap()
        .render_current();
    assert!(storage_message.contains("/sensitive/mount"));
    for unrelated_detail in [
        "secret-label",
        "secret-iface",
        "secret-display",
        "192.0.2.99",
        "2001:db8::99",
    ] {
        assert!(!storage_message.contains(unrelated_detail));
    }
    assert_eq!(state.app.notification_center().alert_count(), 1);
    assert_eq!(
        state.app.notification_center().alert_tone(),
        Some(ui::NotificationTone::Warning)
    );
    state.apply_system_status_snapshot(system_status_test_snapshot(
        11,
        system_services::StoragePressure::Low,
        false,
        system_services::SystemVolumeSource::Detected,
    ));
    let network_message = state
        .app
        .notification_center()
        .alert_message_for_key("system-status.network")
        .unwrap()
        .render_current();
    assert_eq!(network_message, "Network connection was lost");
    for secret in [
        "/sensitive/mount",
        "secret-label",
        "secret-iface",
        "secret-display",
        "192.0.2.99",
        "2001:db8::99",
    ] {
        assert!(!network_message.contains(secret));
    }
    assert_eq!(state.app.notification_center().alert_count(), 2);
    assert_eq!(
        state.app.notification_center().alert_tone(),
        Some(ui::NotificationTone::Warning)
    );
    state.apply_system_status_snapshot(system_status_test_snapshot(
        12,
        system_services::StoragePressure::Low,
        false,
        system_services::SystemVolumeSource::Detected,
    ));
    assert_eq!(state.app.notification_center().alert_count(), 2);
    assert_eq!(
        state.app.notification_center().alert_tone(),
        Some(ui::NotificationTone::Warning)
    );
    state.apply_system_status_snapshot(system_status_test_snapshot(
        13,
        system_services::StoragePressure::Low,
        true,
        system_services::SystemVolumeSource::Detected,
    ));
    assert!(
        state
            .app
            .notification_center()
            .alert_message_for_key("system-status.network")
            .is_none()
    );
    assert_eq!(state.app.notification_center().alert_count(), 1);
    state.apply_system_status_snapshot(system_status_test_snapshot(
        14,
        system_services::StoragePressure::Low,
        false,
        system_services::SystemVolumeSource::Detected,
    ));
    assert!(
        state
            .app
            .notification_center()
            .alert_message_for_key("system-status.network")
            .is_some()
    );
    assert_eq!(state.app.notification_center().alert_count(), 2);
    assert_eq!(
        state.app.notification_center().alert_tone(),
        Some(ui::NotificationTone::Warning)
    );
}

#[test]
fn system_status_focus_clock_roundtrip_and_close_restore_home() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut state, UserRole::Admin);
    state.enter_screen(ShellScreen::SystemStatus);
    state.pages.common.focused_component = ShellComponent::SystemStatus;
    state.refresh_hit_map();
    assert_eq!(state.focus_order(), vec![ShellComponent::SystemStatus]);
    state.open_clock();
    state.close_clock();
    assert_eq!(state.active_screen(), ShellScreen::SystemStatus);
    assert_eq!(state.focused_component(), ShellComponent::SystemStatus);
    state.close_system_status();
    assert_eq!(state.active_screen(), ShellScreen::Home);
    assert_eq!(state.focused_component(), ShellComponent::Home);
}

#[test]
fn system_status_runtime_watch_drain_applies_revision_and_completes_refresh() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut state, UserRole::Admin);
    let initial = system_status_test_snapshot(
        4,
        system_services::StoragePressure::Normal,
        true,
        system_services::SystemVolumeSource::Detected,
    );
    state.apply_system_status_snapshot(initial.clone());
    state
        .pages
        .system_status
        .system_status_refresh_requested_revision = Some(4);
    let system = |snapshot: app::AppSystemStatusSnapshot| system_services::SystemSnapshot {
        revision: snapshot.revision,
        observed_at: snapshot.observed_at,
        weather: system_services::WeatherState::Loading,
        time: system_services::TimeState::Local {
            local_time: snapshot.observed_at.fixed_offset(),
        },
        storage: snapshot.storage,
        network: snapshot.network,
        metrics: snapshot.metrics,
    };
    let (sender, mut receiver) = tokio::sync::watch::channel(system(initial));
    assert!(!drain_system_status_snapshot(&mut receiver, &mut state));
    let updated = system_status_test_snapshot(
        5,
        system_services::StoragePressure::Critical,
        false,
        system_services::SystemVolumeSource::Detected,
    );
    sender.send(system(updated)).unwrap();
    assert!(drain_system_status_snapshot(&mut receiver, &mut state));
    assert_eq!(state.app.system_status_snapshot().unwrap().revision, 5);
    assert!(
        state
            .pages
            .system_status
            .system_status_refresh_requested_revision
            .is_none()
    );
}

#[test]
fn system_status_live_service_home_open_refresh_and_background_close() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "tundra-shell-system-status-live-{}-{unique}",
        std::process::id(),
    ));
    let _temp_guard = SystemStatusTempGuard(root.clone());
    let user_dirs = platform::UserDirs::new(
        root.join("Desktop"),
        root.join("Documents"),
        root.join("Downloads"),
        root.join("Pictures"),
        root.join("Videos"),
        root.join("Music"),
        root.join("UserData"),
    )
    .unwrap();
    let app_paths = platform::build_linux_app_paths(
        root.join("Config"),
        root.join("Data"),
        root.join("Cache"),
        root.join("State"),
        root.join("Temp"),
    )
    .unwrap();
    let opened_storage = StorageManager::open(app_paths.clone()).unwrap();
    let manager = opened_storage.manager.clone();
    // This exercises local status refresh, not network time or geolocation.
    // Keep startup and later settings reconfiguration on the same offline fixture.
    let mut storage_config = manager.load_config().unwrap();
    storage_config.time_sync.source = storage::TimeSyncSource::OperatingSystem;
    manager.save_config(&storage_config).unwrap();
    let session = seed_system_profile(
        &manager,
        "StatusAdmin",
        storage::AppearanceConfig::default(),
    );
    let platform = Arc::new(platform::mock::MockPlatform::new(user_dirs, app_paths));
    platform.set_local_volumes_result(Ok(vec![platform::LocalVolume {
        root: PathBuf::from("/live-system"),
        label: Some("Live".into()),
        kind: platform::VolumeKind::Fixed,
        total_bytes: Some(10 * 1024_u64.pow(3)),
        available_bytes: Some(6 * 1024_u64.pow(3)),
        is_system: true,
        access: platform::VolumeAccess::ReadWrite,
    }]));
    let watchdog = default_editor_watchdog()
        .expect("watchdog")
        .child_component(watchdog::ComponentId::new("system-status-live-test").unwrap());
    let config = system_services_config_for_storage_config(
        &system_services::SystemServicesConfig::default(),
        &storage_config,
    );
    let (handle, mut receiver) =
        system_services::SystemServicesRuntime::start_with_platform_and_provider(
            config.clone(),
            watchdog.clone(),
            platform.clone(),
            Arc::new(SystemStatusTestWeatherProvider),
        );
    let _service_guard = SystemStatusServiceGuard(Some(handle.clone()));
    let mut startup = ShellStartupState::clean(
        PlatformKind::Linux,
        PlatformCapabilities::native_supported(),
    );
    startup.storage_manager = Some(manager.clone());
    let mut state = ShellSession::new_with_runtime_services(
        ShellLaunchConfig::default(),
        (120, 40),
        startup,
        ui::RuntimeAsciiAssets::load_default().unwrap(),
        ShellRuntimeServices {
            window_owner: Default::default(),
            inherited_auth: None,
            session_snapshot: None,
            language: None,
            terminal_image_support: false,
            explorer: None,
            diagnostics: None,
            editor: ShellEditorTaskRuntime::unavailable(),
            settings: ShellSettingsTaskRuntime::new_managed_with_system_services(
                watchdog,
                Some(handle),
                config,
                None,
            ),
        },
        None,
    );
    state.attach_system_session(session);
    state.set_navigation_path(vec![ShellScreen::Home]);
    state.pages.common.focused_component = ShellComponent::Home;
    let index = state
        .user_home_entries()
        .iter()
        .position(|entry| entry.label == "System Status")
        .unwrap();
    state.select_home_entry(index);
    let baseline_revision = receiver.borrow_and_update().revision;
    state.apply_input(InputEvent::from_key_label("Enter"));
    assert_eq!(state.active_screen(), ShellScreen::SystemStatus);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                receiver.changed().await.unwrap();
                apply_current_system_status_snapshot(&mut receiver, &mut state);
                if state
                    .app
                    .system_status_snapshot()
                    .is_some_and(|snapshot| snapshot.revision > baseline_revision)
                {
                    break;
                }
            }
        })
        .await
        .expect("active status sample timed out");
    });
    let opened_revision = state
        .app
        .system_status_snapshot()
        .expect("active sample")
        .revision;
    state.apply_input(InputEvent::from_key_label("R"));
    assert_eq!(
        state
            .pages
            .system_status
            .system_status_refresh_requested_revision,
        Some(opened_revision)
    );
    runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(3), async {
            while state
                .pages
                .system_status
                .system_status_refresh_requested_revision
                .is_some()
            {
                receiver.changed().await.unwrap();
                apply_current_system_status_snapshot(&mut receiver, &mut state);
            }
        })
        .await
        .expect("refreshed status sample timed out");
    });
    assert!(
        state
            .pages
            .system_status
            .system_status_refresh_requested_revision
            .is_none()
    );
    assert!(state.app.system_status_snapshot().unwrap().revision > opened_revision);
    state.apply_input(InputEvent::from_key_label("Esc"));
    assert_eq!(state.active_screen(), ShellScreen::SystemStatus);
    state.return_from_screen(ShellScreen::SystemStatus);
    assert_eq!(state.active_screen(), ShellScreen::Home);

    state.open_settings();
    for key in ["Shift+Tab", "Shift+Tab", "Down", "Down", "Enter"] {
        state.apply_input(InputEvent::from_key_label(key));
    }
    assert_eq!(
        state.to_settings_view_model().unwrap().selected_category,
        ui::SettingsCategory::System
    );
    assert!(
        matches!(&state.app.system_status_snapshot().unwrap().storage, system_services::StorageState::Ready(storage) if storage.overall_pressure == system_services::StoragePressure::Normal)
    );
    // Skip authorization, external control and screen timeout to select storage.
    state.apply_input(InputEvent::from_key_label("Down"));
    state.apply_input(InputEvent::from_key_label("Down"));
    state.apply_input(InputEvent::from_key_label("Down"));
    assert_eq!(
        state.to_settings_view_model().unwrap().selected_field,
        ui::SettingsField::SystemLowAvailable
    );
    state.apply_input(InputEvent::from_key_label("Enter"));
    state.apply_input(InputEvent::from_key_label("Ctrl+A"));
    state.apply_input(InputEvent::from_key_label("6"));
    state.apply_input(InputEvent::from_key_label("Enter"));
    assert_eq!(
        manager
            .load_config()
            .unwrap()
            .system_status
            .low_available_gib,
        6
    );
    let changed_from = state.app.system_status_snapshot().unwrap().revision;
    runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop { receiver.changed().await.unwrap(); apply_current_system_status_snapshot(&mut receiver, &mut state); if state.app.system_status_snapshot().is_some_and(|snapshot| snapshot.revision > changed_from && matches!(&snapshot.storage, system_services::StorageState::Ready(storage) if storage.overall_pressure == system_services::StoragePressure::Low)) { break; } }
        }).await.expect("threshold reconfigure sample timed out");
    });
    state.request_settings_restore_defaults();
    state.apply_input(InputEvent::from_key_label("R"));
    assert_eq!(
        manager.load_config().unwrap().system_status,
        storage::SystemStatusConfig::default()
    );
    let restored_from = state.app.system_status_snapshot().unwrap().revision;
    runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop { receiver.changed().await.unwrap(); apply_current_system_status_snapshot(&mut receiver, &mut state); if state.app.system_status_snapshot().is_some_and(|snapshot| snapshot.revision > restored_from && matches!(&snapshot.storage, system_services::StorageState::Ready(storage) if storage.overall_pressure == system_services::StoragePressure::Normal)) { break; } }
        }).await.expect("restored threshold sample timed out");
    });
    drop(state);
}

#[test]
fn system_overview_metrics_do_not_depend_on_dashboard_placements() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut state, UserRole::User);
    state.apply_system_status_snapshot(system_status_metric_snapshot(10));
    let mut config = state.system_status_dashboard_config();
    let kinds = config.widgets.clone();
    for kind in kinds {
        if kind != storage::SystemStatusWidgetKind::SystemOverview {
            config.remove_widget(kind);
        }
    }
    state.app.dispatch_at(
        app::AppCommand::SetActiveSystemStatusDashboard(Some(config)),
        Instant::now(),
    );
    state.pages.system_status.system_status_route =
        ui::SystemStatusRoute::Detail(ui::SystemStatusDetail::Overview);
    let model = state.to_system_status_view_model().unwrap();
    assert_eq!(model.dashboard.wide_widgets.len(), 1);
    assert_eq!(model.dashboard.overview_metrics.len(), 8);
    let cpu = &model.dashboard.overview_metrics[0];
    assert_eq!(cpu.kind, ui::SystemStatusWidgetKind::Cpu);
    assert!(cpu.progress_percent.is_some());
    assert!(!cpu.trend.as_ref().unwrap().is_empty());
    assert_eq!(
        model.dashboard.overview_metrics[1].kind,
        ui::SystemStatusWidgetKind::Memory
    );
    assert!(
        model.dashboard.overview_metrics[1]
            .progress_percent
            .is_some()
    );
}

#[test]
fn unrelated_system_metrics_keep_static_pages_idle_but_alert_changes_invalidate() {
    use system_services::{StoragePressure, SystemVolumeSource};
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut state, UserRole::User);
    state.enter_screen(ShellScreen::Settings);
    let generation = state.hit_map().generation();
    for revision in [1, 2] {
        assert!(
            !state.apply_system_status_snapshot(system_status_test_snapshot(
                revision,
                StoragePressure::Normal,
                true,
                SystemVolumeSource::Detected,
            ))
        );
    }
    assert_eq!(state.app.system_status_snapshot().unwrap().revision, 2);
    assert_eq!(state.hit_map().generation(), generation);
    assert!(
        state.apply_system_status_snapshot(system_status_test_snapshot(
            3,
            StoragePressure::Low,
            true,
            SystemVolumeSource::Detected,
        ))
    );
    assert!(
        !state.apply_system_status_snapshot(system_status_test_snapshot(
            4,
            StoragePressure::Low,
            true,
            SystemVolumeSource::Detected,
        ))
    );
    assert!(
        state.apply_system_status_snapshot(system_status_test_snapshot(
            5,
            StoragePressure::Critical,
            false,
            SystemVolumeSource::Detected,
        ))
    );
    assert!(
        state.apply_system_status_snapshot(system_status_test_snapshot(
            6,
            StoragePressure::Normal,
            true,
            SystemVolumeSource::Detected,
        ))
    );
    state.enter_screen(ShellScreen::SystemStatus);
    assert!(
        state.apply_system_status_snapshot(system_status_test_snapshot(
            7,
            StoragePressure::Normal,
            true,
            SystemVolumeSource::Detected,
        ))
    );
}
