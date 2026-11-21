use super::*;

fn id(screen: &str, focus: &str, overlay: Option<&str>) -> RedrawIdentity {
    RedrawIdentity {
        screen_saver_active: false,
        language_generation: 1,
        screen: screen.into(),
        focus: focus.into(),
        keyboard_focus_visible: false,
        overlay: overlay.map(|id| RedrawOverlayIdentity {
            kind: if id.contains("toast") {
                ui::MotionOverlayKind::Toast
            } else if id.contains("popover") {
                ui::MotionOverlayKind::Popover
            } else {
                ui::MotionOverlayKind::Dialog
            },
            id: id.to_string(),
        }),
    }
}

#[test]
fn initial_idle_and_state_clock_are_event_driven() {
    let origin = Instant::now();
    let mut scheduler = RedrawScheduler::new(origin, id("home", "one", None), false);
    scheduler.set_state_clock(origin, true);
    assert!(scheduler.is_due(origin));
    scheduler.did_draw(origin);
    assert!(!scheduler.is_due(origin + Duration::from_millis(999)));
    assert_eq!(
        scheduler.poll_timeout(origin, Duration::MAX),
        Duration::from_secs(1)
    );
    assert!(scheduler.is_due(origin + Duration::from_secs(1)));
    scheduler.did_draw(origin + Duration::from_secs(1));
    assert!(!scheduler.is_due(origin + Duration::from_secs(1)));
}

#[test]
fn header_clock_redraws_at_minute_changes_and_stops_sampling_when_hidden() {
    let origin = Instant::now();
    let mut scheduler = RedrawScheduler::new(origin, id("home", "one", None), false);
    let date = chrono::NaiveDate::from_ymd_opt(2026, 1, 5).unwrap();
    let display = |date: chrono::NaiveDate, seconds: u32| time::ClockDisplay {
        date,
        time: chrono::NaiveTime::from_num_seconds_from_midnight_opt(seconds, 0).unwrap(),
        warning: None,
    };
    scheduler.observe_header_clock(origin, true, || display(date, 30));
    scheduler.did_draw(origin);
    assert_eq!(
        scheduler.poll_timeout(origin, Duration::MAX),
        Duration::from_secs(1)
    );
    scheduler.observe_header_clock(origin + Duration::from_millis(500), true, || {
        panic!("early wake should not read the clock")
    });

    let mut redraw_seconds = Vec::new();
    for second in 1..=60 {
        let now = origin + Duration::from_secs(second);
        scheduler.observe_header_clock(now, true, || display(date, 30 + second as u32));
        if scheduler.is_due(now) {
            redraw_seconds.push(second);
            scheduler.did_draw(now);
        }
    }
    assert_eq!(redraw_seconds, [30]);

    let now = origin + Duration::from_secs(61);
    let next_date = date.succ_opt().unwrap();
    scheduler.observe_header_clock(now, true, || display(next_date, 90));
    assert!(scheduler.is_due(now));
    scheduler.did_draw(now);

    scheduler.observe_header_clock(now, false, || panic!("hidden clock should not be read"));
    assert_eq!(scheduler.poll_timeout(now, Duration::MAX), Duration::MAX);
    assert!(!scheduler.is_due(now + Duration::from_secs(60)));

    scheduler.observe_header_clock(now, true, || display(next_date, 90));
    assert!(scheduler.is_due(now));
}

#[test]
fn all_shell_identity_changes_redraw_immediately_without_transitions() {
    let origin = Instant::now();
    for changed in [
        id("settings", "one", None),
        id("home", "two", None),
        RedrawIdentity {
            keyboard_focus_visible: true,
            ..id("home", "one", None)
        },
        id("home", "one", Some("dialog")),
        id("home", "one", Some("popover:menu")),
        id("home", "one", Some("toast:notice")),
    ] {
        let mut scheduler = RedrawScheduler::new(origin, id("home", "one", None), false);
        scheduler.did_draw(origin);
        scheduler.request_animation_frame(origin);

        scheduler.observe(origin, changed, false);

        assert!(scheduler.is_due(origin));
        assert_eq!(
            scheduler.transitions(origin),
            ui::MotionTransitions::default()
        );
        scheduler.did_draw(origin);
        assert!(!scheduler.is_due(origin + FRAME_INTERVAL));
    }
}

#[test]
fn reduced_motion_is_forwarded_to_independent_widgets() {
    let origin = Instant::now();
    let scheduler = RedrawScheduler::new(origin, id("home", "one", None), true);
    let frame = scheduler.frame(origin, 125);
    assert!(frame.reduced_motion);
    assert_eq!(frame.animation_speed_percent, 125);
}

#[test]
fn independent_widget_animation_can_request_the_next_frame() {
    let origin = Instant::now();
    let mut scheduler = RedrawScheduler::new(origin, id("home", "one", None), false);
    scheduler.did_draw(origin);
    scheduler.request_animation_frame(origin);
    assert_eq!(
        scheduler.poll_timeout(origin, Duration::MAX),
        FRAME_INTERVAL
    );
    assert!(!scheduler.is_due(origin + Duration::from_millis(16)));
    assert!(scheduler.is_due(origin + FRAME_INTERVAL));
}

#[test]
fn continuous_color_frames_keep_their_clock_when_motion_is_reduced() {
    let origin = Instant::now();
    let identity = id("editor", "canvas", None);
    let mut scheduler = RedrawScheduler::new(origin, identity.clone(), true);
    scheduler.did_draw(origin);
    scheduler.request_animation_frame(origin);
    assert_eq!(scheduler.poll_timeout(origin, Duration::MAX), Duration::MAX);

    for frame_number in 0..3 {
        let now = origin + CONTINUOUS_FRAME_INTERVAL * frame_number;
        scheduler.request_continuous_frame(now);
        scheduler.observe(now, identity.clone(), true);
        assert_eq!(
            scheduler.poll_timeout(now, Duration::MAX),
            CONTINUOUS_FRAME_INTERVAL
        );
        assert!(!scheduler.is_due(now + Duration::from_millis(33)));
        let next = now + CONTINUOUS_FRAME_INTERVAL;
        assert!(scheduler.is_due(next));
        for speed in [0, 25, 100, 200] {
            let frame = scheduler.frame(next, speed);
            assert_eq!(frame.now, next.duration_since(origin));
            assert!(frame.scaled_delta().is_zero());
        }
        scheduler.did_draw(next);
    }
    assert!(!scheduler.is_due(origin + CONTINUOUS_FRAME_INTERVAL * 4));
}

#[test]
fn urgent_input_bypasses_animation_budget_while_background_updates_coalesce() {
    let origin = Instant::now();
    let mut scheduler = RedrawScheduler::new(origin, id("home", "one", None), false);
    scheduler.did_draw(origin);
    scheduler.request_animation_frame(origin);
    scheduler.request_redraw();
    assert!(!scheduler.is_due(origin + Duration::from_millis(1)));
    scheduler.request_input_redraw();
    assert!(scheduler.is_due(origin + Duration::from_millis(1)));
    assert_eq!(
        scheduler.poll_timeout(origin, Duration::MAX),
        Duration::ZERO
    );
}

#[test]
fn demand_uses_nearest_deadline_and_slow_output_never_replays_frame_backlog() {
    let origin = Instant::now();
    let mut scheduler = RedrawScheduler::new(origin, id("home", "one", None), false);
    scheduler.did_draw(origin);
    let completed = origin + Duration::from_millis(80);
    scheduler.request_animation_frame_in(completed, Duration::from_millis(2));
    scheduler.request_frame_after(completed, Duration::from_millis(80));
    assert_eq!(
        scheduler.poll_timeout(completed, Duration::MAX),
        Duration::from_millis(2)
    );
    assert!(!scheduler.is_due(completed));
    assert!(scheduler.is_due(completed + Duration::from_millis(2)));
    scheduler.did_draw(completed + Duration::from_millis(2));
    assert_eq!(
        scheduler.poll_timeout(completed + Duration::from_millis(2), Duration::MAX),
        Duration::MAX
    );
}

#[test]
fn slow_output_skips_missed_state_clock_ticks_but_preserves_elapsed_motion_time() {
    let origin = Instant::now();
    let mut scheduler = RedrawScheduler::new(origin, id("home", "one", None), false);
    scheduler.set_state_clock(origin, true);
    scheduler.did_draw_finished(origin, origin + Duration::from_millis(2_500));
    let next = origin + Duration::from_millis(2_500) + FRAME_INTERVAL;
    scheduler.request_animation_frame(origin + Duration::from_millis(2_500));
    assert_eq!(
        scheduler.poll_timeout(origin + Duration::from_millis(2_500), Duration::MAX),
        FRAME_INTERVAL
    );
    assert_eq!(
        scheduler.frame(next, 100).delta,
        Duration::from_millis(2_500) + FRAME_INTERVAL
    );
}

#[test]
fn static_app_windows_have_no_implicit_clock_deadline() {
    let origin = Instant::now();
    let mut scheduler = RedrawScheduler::new(origin, id("settings", "one", None), false);
    scheduler.did_draw(origin);
    assert_eq!(scheduler.poll_timeout(origin, Duration::MAX), Duration::MAX);
    assert!(!scheduler.is_due(origin + Duration::from_secs(60)));
    scheduler.set_state_clock(origin, true);
    assert!(scheduler.is_due(origin + Duration::from_secs(1)));
    scheduler.set_state_clock(origin, false);
    assert!(!scheduler.is_due(origin + Duration::from_secs(1)));
}
