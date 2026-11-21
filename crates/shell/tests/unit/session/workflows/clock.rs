use super::*;

#[test]
fn clock_management_enter_cancels_without_deleting_or_changing_entries() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    let now = Instant::now();
    let mut clock = ClockScheduler::default();
    let alarm = clock
        .create_daily_alarm("07 30 00", &state.app.snapshot().clock)
        .unwrap();
    let countdown = clock
        .create_countdown("00 01 00", &state.app.snapshot().clock, now)
        .unwrap();
    state.clock_scheduler = Some(clock);
    let before = state.clock_scheduler.as_ref().unwrap().entries(now);
    for id in [alarm, countdown] {
        state.show_clock_manage_dialog(id);
        let modal = state.to_notification_view_model().unwrap();
        assert_eq!(
            modal
                .actions
                .iter()
                .find(|action| action.selected)
                .unwrap()
                .id,
            "cancel"
        );
        state.apply_input(InputEvent::key(InputKey::Enter));
        assert!(state.to_notification_view_model().is_none());
        assert_eq!(state.clock_scheduler.as_ref().unwrap().entries(now), before);
        state.apply_input(InputEvent::key(InputKey::Down));
    }
}

#[test]
fn strong_clock_due_events_keep_reminder_semantics_and_dismiss_actions() {
    for (kind, title) in [
        (ScheduledClockEntryKind::DailyAlarm, "Alarm reminder"),
        (ScheduledClockEntryKind::Countdown, "Countdown reminder"),
    ] {
        let mut state = ShellSession::new(ShellLaunchConfig::default(), (116, 41));
        set_test_auth_role(&mut state, UserRole::User);
        while state.notification_dismiss_active_modal_without_response() {}
        let ordinary = state.handle_clock_due_events(vec![DueEvent {
            id: 1,
            kind,
            strong: true,
            display_time: "07:34:56".into(),
        }]);
        assert!(ordinary.is_none());
        let model = state.to_notification_view_model().unwrap();
        assert_eq!(model.tone, ui::NotificationTone::Info);
        assert_eq!(model.title, title);
        assert!(model.actions.iter().any(|action| action.id == "dismiss"));
        state.apply_input(InputEvent::from_key_label("Esc"));
        assert!(state.to_notification_view_model().is_none());
    }
}

#[test]
fn clock_storage_retry_keeps_the_due_summary_visible() {
    let mut state = ShellSession::new(ShellLaunchConfig::default(), (80, 24));
    state.remember_clock_due_summary("Countdown finished".to_string());

    state.report_clock_storage_error("first failure");
    state.report_clock_storage_error("retry failure");

    assert!(
        state
            .to_shell_chrome_view_model()
            .status
            .error
            .as_deref()
            .is_some_and(|message| {
                message.contains("Countdown finished") && message.contains("retry failure")
            })
    );
}

#[test]
fn clock_cancel_is_reachable_by_keyboard_and_accepts_enter_space_or_escape() {
    for key in ["Enter", " ", "Esc"] {
        let mut state = ShellSession::new_for_home_mode(
            ShellLaunchConfig::default(),
            (120, 40),
            ShellHomeMode::User,
        );
        state.set_navigation_path(vec![ShellScreen::Clock]);
        state.pages.clock.clock_create_state = Some(ClockCreateState::default());
        state.refresh_hit_map();
        state.apply_input(InputEvent::from_key_label("Shift+Tab"));
        assert_eq!(
            state.pages.common.focused_component,
            ShellComponent::ClockCreateCancelButton
        );
        state.apply_input(InputEvent::from_key_label(key));
        assert!(state.pages.clock.clock_create_state.is_none(), "{key}");
    }
}

#[test]
fn clock_create_keyboard_edits_three_fields_and_cycles_actions() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (100, 30),
        ShellHomeMode::User,
    );
    state.set_navigation_path(vec![ShellScreen::Clock]);
    state.pages.clock.clock_create_state = Some(ClockCreateState::default());
    state.refresh_hit_map();
    for key in ["2", "3", "Down", "5", "9", "Down", "5", "9"] {
        state.apply_input(InputEvent::from_key_label(key));
    }
    assert_eq!(
        state
            .pages
            .clock
            .clock_create_state
            .as_ref()
            .unwrap()
            .values,
        [23, 59, 59]
    );
    state.apply_input(InputEvent::from_key_label("Up"));
    assert_eq!(
        state
            .pages
            .clock
            .clock_create_state
            .as_ref()
            .unwrap()
            .values,
        [23, 59, 59]
    );
    state.apply_input(InputEvent::from_key_label("Down"));
    assert_eq!(
        state
            .pages
            .clock
            .clock_create_state
            .as_ref()
            .unwrap()
            .values,
        [23, 59, 59]
    );
    state.apply_input(InputEvent::from_key_label("Left"));
    state.apply_input(InputEvent::from_key_label("Backspace"));
    state.apply_input(InputEvent::from_key_label("4"));
    state.apply_input(InputEvent::from_key_label("Tab"));
    assert_eq!(
        state
            .pages
            .clock
            .clock_create_state
            .as_ref()
            .unwrap()
            .values,
        [23, 59, 49]
    );
    assert_eq!(
        state.pages.common.focused_component,
        ShellComponent::ClockCreateAlarmButton
    );
    state.apply_input(InputEvent::from_key_label("Tab"));
    assert_eq!(
        state.pages.common.focused_component,
        ShellComponent::ClockCreateCountdownButton
    );
    state.apply_input(InputEvent::from_key_label("Tab"));
    assert_eq!(
        state.pages.common.focused_component,
        ShellComponent::ClockCreateCancelButton
    );
    state.apply_input(InputEvent::from_key_label("Tab"));
    assert_eq!(
        state
            .pages
            .clock
            .clock_create_state
            .as_ref()
            .unwrap()
            .active_field,
        0
    );
    state.apply_input(InputEvent::from_key_label("2"));
    state.apply_input(InputEvent::from_key_label("9"));
    assert_eq!(
        state
            .pages
            .clock
            .clock_create_state
            .as_ref()
            .unwrap()
            .values[0],
        2
    );
    state.apply_input(InputEvent::from_key_label("3"));
    assert_eq!(
        state
            .pages
            .clock
            .clock_create_state
            .as_ref()
            .unwrap()
            .values[0],
        23
    );
    state.apply_input(InputEvent::from_key_label("Esc"));
    assert!(state.pages.clock.clock_create_state.is_none());
}

#[test]
fn clock_create_arrows_are_clickable_once_and_fields_accept_replacement_digits() {
    for size in [(100, 30), (50, 12)] {
        let mut state = ShellSession::new_for_home_mode(
            ShellLaunchConfig::default(),
            size,
            ShellHomeMode::User,
        );
        state.set_navigation_path(vec![ShellScreen::Clock]);
        state.pages.clock.clock_create_state = Some(ClockCreateState::default());
        state.refresh_hit_map();
        let ui::ShellLayout::Full { main, .. } =
            state.shell_layout_for(Rect::new(0, 0, size.0, size.1))
        else {
            panic!("full layout");
        };
        let layout = ui::clock_page_layout(main, &state.to_clock_view_model())
            .create_dialog
            .unwrap();
        for field in 0..3 {
            for (area, expected) in [
                (layout.decrements[field], if field == 0 { 23 } else { 59 }),
                (layout.increments[field], 0),
            ] {
                assert_eq!(area.height, 1);
                let point = (area.x + area.width / 2, area.y);
                state.apply_input(InputEvent::mouse_down(PointerButton::Left, point));
                state.apply_input(InputEvent::mouse_up(PointerButton::Left, point));
                let create = state.pages.clock.clock_create_state.as_ref().unwrap();
                assert_eq!(create.values[field], expected);
                assert_eq!(create.active_field, field);
            }
            let area = layout.values[field];
            let point = (area.x + area.width / 2, area.y);
            state.apply_input(InputEvent::mouse_down(PointerButton::Left, point));
            state.apply_input(InputEvent::mouse_up(PointerButton::Left, point));
            for digit in ['1', '2'] {
                state.apply_input(InputEvent::key(InputKey::Char(digit)));
            }
            assert_eq!(
                state
                    .pages
                    .clock
                    .clock_create_state
                    .as_ref()
                    .unwrap()
                    .values[field],
                12
            );
        }
    }
}

#[test]
fn compact_clock_keeps_visible_controls_and_keyboard_access() {
    let mut state = ShellSession::new(ShellLaunchConfig::default(), (49, 11));
    state.set_navigation_path(vec![ShellScreen::Clock]);
    set_test_auth_role(&mut state, UserRole::User);

    assert_eq!(
        state.route_clock_key(&KeyInput::from_label("n")).1,
        ShellCommand::ClockOpenCreate
    );
    assert_eq!(state.focus_order(), vec![ShellComponent::ClockNewButton]);

    state.pages.clock.clock_create_state = Some(ClockCreateState::default());
    assert_eq!(
        state.route_clock_key(&KeyInput::from_label("Esc")).1,
        ShellCommand::ClockCreateFormInput(InputEvent::from_key_label("Esc"))
    );
}

#[test]
fn cached_time_sync_replays_into_recreated_shell_state() {
    let received_at = Instant::now();
    let consumed_at = received_at + Duration::from_secs(3);
    let replayed_at = received_at + Duration::from_secs(5);
    let utc = Utc::now();
    let mut cached = None;
    let mut original_state = ShellSession::new(ShellLaunchConfig::default(), (80, 24));

    apply_timed_time_sync_result_at(
        &mut original_state,
        &mut cached,
        TimedTimeSyncResult {
            result: Ok(utc),
            received_at,
        },
        consumed_at,
    );

    assert_eq!(
        original_state.last_time_sync_utc,
        Some(utc + Duration::from_secs(3))
    );

    let mut recreated_state = ShellSession::new(ShellLaunchConfig::default(), (80, 24));
    cached
        .as_ref()
        .expect("successful sync should be cached")
        .apply_to_state_at(&mut recreated_state, replayed_at);

    assert!(recreated_state.time_sync_attempted);
    assert_eq!(
        recreated_state.last_time_sync_utc,
        Some(utc + Duration::from_secs(5))
    );

    let mut failed_state = ShellSession::new(ShellLaunchConfig::default(), (80, 24));
    CachedTimeSyncResult::Failure.apply_to_state_at(&mut failed_state, replayed_at);
    assert!(failed_state.time_sync_attempted);
    assert!(!failed_state.time_sync_failure_dialog_visible());
    assert!(failed_state.status().contains("system clock"));
}

#[test]
fn clock_button_routes_before_explorer_popup_and_app_forms() {
    let mut explorer = explorer_routing_test_state();
    let clock = hit_region_center(&explorer, ShellComponent::ClockButton);
    explorer.pages.common.active_popup = Some(ShellPopup {
        owner: Some(ShellComponent::Explorer),
        anchor: (10, 10),
    });
    explorer.pages.explorer.explorer_overlay_mode =
        Some(ExplorerOverlayMode::ContextMenu { anchor: (10, 10) });
    explorer.refresh_hit_map();

    let routed = explorer.route_input_at(
        InputEvent::mouse_down(PointerButton::Left, clock),
        Instant::now(),
    );
    assert_eq!(
        routed.target,
        RoutedTarget::Component(ShellComponent::ClockButton)
    );
    assert_eq!(routed.command, ShellCommand::OpenClock);

    explorer.pages.common.active_popup = None;
    explorer.pages.explorer.explorer_overlay_mode = None;
    explorer.pages.explorer.explorer_input_mode = ExplorerInputMode::NewFolder;
    explorer.refresh_hit_map();
    let routed = explorer.route_input_at(
        InputEvent::mouse_down(PointerButton::Left, clock),
        Instant::now(),
    );
    assert_eq!(routed.command, ShellCommand::OpenClock);
}

#[test]
fn clock_button_routes_outside_shell_modal_while_modal_region_stays_highest() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    state.notify_modal(
        "Confirm",
        "Keep the clock available",
        ui::NotificationTone::Info,
        vec![ShellNotificationAction::new("ok", "OK")],
    );
    let clock = hit_region_center(&state, ShellComponent::ClockButton);
    let dialog = hit_region_center(&state, ShellComponent::NotificationDialog);

    assert_eq!(
        state.hit_map.layer_at(clock),
        Some(ShellHitLayer::ShellChrome)
    );
    assert_eq!(
        state.hit_map.layer_at(dialog),
        Some(ShellHitLayer::ShellModal)
    );

    let routed = state.route_input_at(
        InputEvent::mouse_down(PointerButton::Left, clock),
        Instant::now(),
    );
    assert_eq!(routed.command, ShellCommand::OpenClock);
}
