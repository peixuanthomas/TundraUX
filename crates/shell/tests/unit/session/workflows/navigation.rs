use super::*;

#[test]
fn command_line_open_requires_size_and_routes_ctrl_c_to_the_child() {
    let mut state = ShellSession::new(ShellLaunchConfig::default(), (120, 40));
    set_test_auth_role(&mut state, UserRole::Admin);
    state.set_navigation_path(vec![ShellScreen::Home, ShellScreen::Launcher]);
    state.open_command_line();

    assert_eq!(state.active_screen(), ShellScreen::CommandLine);
    assert_eq!(state.focused_component(), ShellComponent::CommandLine);
    let ctrl_c = ui::KeyEvent::with_modifiers(ui::Key::Char('c'), ui::KeyModifiers::CTRL);
    let (_, command) = state.route_key_input(&ctrl_c);
    assert_eq!(command, ShellCommand::CommandLineKey(ctrl_c));
    assert!(!state.shutdown_requested());

    state.close_command_line();
    assert_eq!(state.active_screen(), ShellScreen::Launcher);

    state.terminal_size = (107, 22);
    state.open_command_line();
    assert_eq!(state.active_screen(), ShellScreen::Launcher);
}

#[test]
fn command_line_keeps_the_shell_clock_button_visible_and_clickable() {
    let mut state = ShellSession::new(ShellLaunchConfig::default(), (120, 40));
    set_test_auth_role(&mut state, UserRole::Admin);
    state.set_navigation_path(vec![ShellScreen::Home, ShellScreen::Launcher]);
    state.open_command_line();

    assert!(
        state
            .to_shell_chrome_view_model()
            .status
            .time_button_label
            .is_some()
    );
    let clock = hit_region_center(&state, ShellComponent::ClockButton);
    assert_eq!(
        state.hit_map.layer_at(clock),
        Some(ShellHitLayer::ShellChrome)
    );

    let routed = state.route_input_at(
        InputEvent::mouse_down(PointerButton::Left, clock),
        Instant::now(),
    );
    assert_eq!(
        routed.target,
        RoutedTarget::Component(ShellComponent::ClockButton)
    );
    assert_eq!(routed.command, ShellCommand::OpenClock);
}

#[test]
fn back_button_routes_the_same_command_as_escape() {
    for size in [(120, 40), (49, 11), (30, 8)] {
        let mut state = ShellSession::new(ShellLaunchConfig::default(), size);
        set_test_auth_role(&mut state, UserRole::Admin);
        while state.notification_dismiss_active_modal_without_response() {}
        for screen in [
            ShellScreen::Clock,
            ShellScreen::Diagnostics,
            ShellScreen::SystemStatus,
            ShellScreen::Logs,
            ShellScreen::Management,
            ShellScreen::Explorer,
            ShellScreen::Launcher,
            ShellScreen::Editor,
            ShellScreen::Settings,
            ShellScreen::ExitConfirm,
        ] {
            let caller = match screen {
                ShellScreen::Clock => ShellScreen::Logs,
                _ => ShellScreen::Clock,
            };
            state.set_navigation_path(vec![ShellScreen::Home, caller, screen]);
            state.refresh_hit_map();
            let back = hit_region_center(&state, ShellComponent::BackButton);
            let click = state.route_input_at(
                InputEvent::mouse_down(PointerButton::Left, back),
                Instant::now(),
            );
            let escape = state.route_input_at(InputEvent::key(InputKey::Escape), Instant::now());
            assert_eq!(click, escape, "{screen:?} at {size:?}");
        }
    }
}

#[test]
fn back_button_cancels_the_overlay_before_leaving_the_page_and_only_activates_once() {
    let mut state = ShellSession::new(ShellLaunchConfig::default(), (120, 40));
    while state.notification_dismiss_active_modal_without_response() {}
    state.set_navigation_path(vec![
        ShellScreen::Home,
        ShellScreen::Logs,
        ShellScreen::Clock,
    ]);
    state.pages.clock.clock_create_state = Some(ClockCreateState::default());
    state.refresh_hit_map();
    let back = hit_region_center(&state, ShellComponent::BackButton);
    state.apply_input(InputEvent::mouse_down(PointerButton::Left, back));
    assert!(state.pages.clock.clock_create_state.is_some());
    state.apply_input(InputEvent::mouse_up(PointerButton::Left, back));
    assert!(state.pages.clock.clock_create_state.is_none());
    assert_eq!(state.active_screen(), ShellScreen::Clock);
    for input in [
        InputEvent::mouse_up(PointerButton::Left, back),
        InputEvent::mouse_down(PointerButton::Right, back),
        InputEvent::mouse_moved(back),
    ] {
        state.apply_input(input);
        assert_eq!(state.active_screen(), ShellScreen::Clock);
    }
    assert!(state.to_shell_chrome_view_model().back_button_hovered);
    let back = hit_region_center(&state, ShellComponent::BackButton);
    state.apply_input(InputEvent::mouse_down(PointerButton::Left, back));
    assert_eq!(state.active_screen(), ShellScreen::Clock);
    state.apply_input(InputEvent::mouse_up(PointerButton::Left, back));
    assert_eq!(state.active_screen(), ShellScreen::Logs);
    assert_ne!(state.focused_component(), ShellComponent::BackButton);

    state.notify_modal(
        "Confirm",
        "Back cancels the modal",
        ui::NotificationTone::Info,
        vec![ShellNotificationAction::new("ok", "OK")],
    );
    let back = hit_region_center(&state, ShellComponent::BackButton);
    let escape = state.route_input_at(InputEvent::key(InputKey::Escape), Instant::now());
    let click = state.route_input_at(
        InputEvent::mouse_down(PointerButton::Left, back),
        Instant::now(),
    );
    assert_eq!(click, escape);
}

#[test]
fn back_button_hit_region_tracks_resize_including_compact_mode() {
    let mut state = ShellSession::new(ShellLaunchConfig::default(), (120, 40));
    while state.notification_dismiss_active_modal_without_response() {}
    state.set_navigation_path(vec![ShellScreen::Home, ShellScreen::Launcher]);
    for (width, height) in [(80, 24), (50, 12), (49, 12), (30, 8), (120, 40)] {
        state.apply_input(InputEvent::Resize { width, height });
        let layout = ui::ShellFrameLayout::new(
            Rect::new(0, 0, width, height),
            None,
            &ui::RenderContext::default(),
        )
        .with_back_label(state.header_back_label().as_deref());
        let hit = state
            .hit_map
            .regions()
            .iter()
            .find(|region| region.component == ShellComponent::BackButton);
        assert_eq!(hit.map(|region| region.area), layout.back_button);
        assert!(hit.is_some());
        if let Some(region) = hit {
            for position in region.area.positions() {
                assert_eq!(
                    state.hit_map.target_at((position.x, position.y)),
                    Some(ShellComponent::BackButton)
                );
            }
        }
    }
}

#[test]
fn focus_navigation_cycles_the_dynamic_home_order_in_both_directions() {
    for size in [(108, 20), (120, 40)] {
        let mut state = ShellSession::new_for_home_mode(
            ShellLaunchConfig::default(),
            size,
            ShellHomeMode::User,
        );
        state.set_navigation_path(vec![ShellScreen::Home]);
        state.pages.common.focused_component = ShellComponent::Home;
        state.refresh_hit_map();

        let expected = [
            ShellComponent::Home,
            ShellComponent::HomeEdit,
            ShellComponent::ClockButton,
            ShellComponent::HelpButton,
            ShellComponent::StatusBar,
            ShellComponent::TopBar,
        ];
        assert_eq!(state.focus_order(), expected);
        let main = state.frame_layout.unwrap().main;
        let edit = ui::home_edit_area(main, &state.to_home_view_model()).unwrap();
        assert!(!edit.is_empty());
        assert!(
            state.hit_map.regions().iter().any(|region| {
                region.component == ShellComponent::HomeEdit && region.area == edit
            })
        );

        for component in expected.into_iter().skip(1).chain([ShellComponent::Home]) {
            state.move_focus(ui::FocusDirection::Next);
            assert_eq!(state.pages.common.focused_component, component);
        }
        for component in expected.into_iter().rev() {
            state.move_focus(ui::FocusDirection::Previous);
            assert_eq!(state.pages.common.focused_component, component);
        }
    }
}

#[test]
fn refresh_hit_map_normalizes_an_illegal_focus_to_the_order_start() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    state.set_navigation_path(vec![ShellScreen::FirstRunSetup]);
    state.pages.common.focused_component = ShellComponent::TopBar;

    state.refresh_hit_map();

    assert_eq!(
        state.pages.common.focused_component,
        ShellComponent::SetupLanguage
    );
}

#[test]
fn focus_navigation_does_not_leave_a_single_component_modal_scope() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    state.notify_modal(
        "Confirm",
        "Stay focused",
        ui::NotificationTone::Info,
        vec![ShellNotificationAction::new("ok", "OK")],
    );

    assert_eq!(
        state.focus_order(),
        vec![ShellComponent::NotificationDialog]
    );
    state.move_focus(ui::FocusDirection::Previous);
    assert_eq!(
        state.pages.common.focused_component,
        ShellComponent::NotificationDialog
    );
    state.move_focus(ui::FocusDirection::Next);
    assert_eq!(
        state.pages.common.focused_component,
        ShellComponent::NotificationDialog
    );
}

#[test]
fn hit_map_uses_explicit_layer_priority_instead_of_insertion_order() {
    let area = Rect::new(0, 0, 10, 5);
    let map = ShellHitMap::new(
        (10, 5),
        1,
        vec![
            ShellHitRegion {
                component: ShellComponent::ExitDialog,
                area,
                layer: ShellHitLayer::ShellModal,
            },
            ShellHitRegion {
                component: ShellComponent::ClockButton,
                area,
                layer: ShellHitLayer::ShellChrome,
            },
            ShellHitRegion {
                component: ShellComponent::ContextMenu,
                area,
                layer: ShellHitLayer::AppOverlay,
            },
            ShellHitRegion {
                component: ShellComponent::Explorer,
                area,
                layer: ShellHitLayer::AppContent,
            },
        ],
    );

    assert_eq!(map.target_at((2, 2)), Some(ShellComponent::ExitDialog));
    assert_eq!(map.layer_at((2, 2)), Some(ShellHitLayer::ShellModal));

    let without_modal = ShellHitMap::new((10, 5), 2, map.regions()[1..].to_vec());
    assert_eq!(
        without_modal.target_at((2, 2)),
        Some(ShellComponent::ClockButton)
    );

    let app_only = ShellHitMap::new((10, 5), 3, map.regions()[2..].to_vec());
    assert_eq!(
        app_only.target_at((2, 2)),
        Some(ShellComponent::ContextMenu)
    );
}

#[test]
fn hit_map_keeps_duplicate_component_regions_distinct() {
    let map = ShellHitMap::new(
        (10, 5),
        1,
        vec![
            ShellHitRegion::new(
                ShellComponent::ClockButton,
                Rect::new(0, 0, 5, 5),
                ShellHitLayer::AppContent,
            ),
            ShellHitRegion::new(
                ShellComponent::ClockButton,
                Rect::new(1, 1, 2, 2),
                ShellHitLayer::ShellChrome,
            ),
        ],
    );

    assert_eq!(map.region_at((0, 0)), Some(&map.regions()[0]));
    assert_eq!(map.region_at((1, 1)), Some(&map.regions()[1]));
}
