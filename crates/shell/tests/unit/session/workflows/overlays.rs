use super::*;

fn overlay_areas_for(state: &ShellSession, component: ShellComponent) -> Vec<Rect> {
    state
        .hit_map()
        .regions()
        .iter()
        .filter(|region| {
            region.component == component
                && matches!(
                    region.layer,
                    ShellHitLayer::AppOverlay | ShellHitLayer::ShellModal
                )
        })
        .map(|region| region.area)
        .collect()
}

#[test]
fn critical_modal_preempts_and_then_restores_the_previous_modal() {
    let mut center = NotificationCenter::new("Ready");
    center.push_modal(ShellNotification::modal(
        "Normal confirmation",
        "normal",
        ui::NotificationTone::Warning,
        vec![ShellNotificationAction::new("ok", "OK")],
    ));
    center.push_critical_modal(ShellNotification::modal(
        "Recovered from panic",
        "critical",
        ui::NotificationTone::Critical,
        vec![ShellNotificationAction::new("continue", "Continue")],
    ));

    assert_eq!(
        center.active_modal_view_model().unwrap().title,
        "Recovered from panic"
    );
    center.activate_selected_action();
    assert_eq!(
        center.active_modal_view_model().unwrap().title,
        "Normal confirmation"
    );
}

#[test]
fn exit_confirmation_keeps_setup_as_the_content_screen() {
    let mut state = ShellSession::new(ShellLaunchConfig::default(), (120, 40));
    state.set_navigation_path(vec![ShellScreen::FirstRunSetup]);
    state.pages.common.focused_component = ShellComponent::SetupLanguage;
    state.refresh_hit_map();

    state.apply_input(InputEvent::from_key_label("Esc"));
    assert_eq!(state.active_screen(), ShellScreen::FirstRunSetup);
    let point = hit_region_center(&state, ShellComponent::TopBar);
    state.apply_input(InputEvent::mouse_down(PointerButton::Left, point));
    let action = state.apply_input(InputEvent::mouse_up(PointerButton::Left, point));

    assert_eq!(action, ShellAction::Redraw);
    assert_eq!(state.active_screen(), ShellScreen::ExitConfirm);
    assert_eq!(state.content_screen(), ShellScreen::FirstRunSetup);
    assert_eq!(
        state.to_shell_chrome_view_model().display_mode,
        ui::HomeDisplayMode::Auth
    );
    assert!(
        state
            .hit_map()
            .regions()
            .iter()
            .any(|region| region.component == ShellComponent::SetupLanguage)
    );
    assert!(
        state
            .hit_map()
            .regions()
            .iter()
            .any(|region| region.component == ShellComponent::ExitDialog)
    );

    state.apply_input(InputEvent::from_key_label("Esc"));

    assert_eq!(state.active_screen(), ShellScreen::FirstRunSetup);
    assert_eq!(state.content_screen(), ShellScreen::FirstRunSetup);
    assert_eq!(state.focused_component(), ShellComponent::SetupLanguage);
}

#[test]
fn motion_keeps_page_and_overlay_hit_regions_fixed_and_immediately_available() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    let transition = |kind, progress| ui::MotionTransition {
        kind,
        direction: ui::MotionDirection::Entering,
        progress,
        phase_progress: progress,
        active: progress < 1_000,
        next_redraw_in: Duration::from_millis(16),
    };
    let bounds = Rect::new(0, 0, 120, 40);
    let settled = ui::ShellFrameLayout::new(
        bounds,
        state.status_time_button_label().as_deref(),
        &ui::RenderContext::default(),
    )
    .with_back_label(state.header_back_label().as_deref())
    .with_help_button(state.help_button_available());
    let settled = state.input_keyboard_content_layout(settled);
    for progress in [0, 250, 499, 500, 1_000] {
        state.refresh_hit_map_with_motion(ui::MotionTransitions {
            screen: Some(transition(ui::MotionTransitionKind::Page, progress)),
            ..Default::default()
        });
        let top = state
            .hit_map()
            .regions()
            .iter()
            .find(|region| region.component == ShellComponent::TopBar)
            .unwrap();
        assert_eq!(top.area, settled.title);
        assert_eq!(state.shell_layout_for(bounds), settled.shell);
        let status = state
            .hit_map()
            .regions()
            .iter()
            .find(|region| region.component == ShellComponent::StatusBar)
            .unwrap();
        assert_eq!(status.area, settled.status_message.unwrap());
        assert_eq!(status.area.y, bounds.y);
        assert!(top.area.intersection(status.area).is_empty());
        for region in [top, status] {
            assert_eq!(
                state.hit_map().target_at((region.area.x, region.area.y)),
                Some(region.component),
            );
        }
    }
    state.apply_input(InputEvent::from_key_label("Esc"));
    let mut dialog_area = None;
    for progress in [0, 250, 499, 500, 1_000] {
        state.refresh_hit_map_with_motion(ui::MotionTransitions {
            overlay: Some(transition(ui::MotionTransitionKind::Dialog, progress)),
            ..Default::default()
        });
        let current = state
            .hit_map()
            .regions()
            .iter()
            .find(|region| region.component == ShellComponent::ExitDialog)
            .expect("first-frame dialog hit area")
            .area;
        assert_eq!(*dialog_area.get_or_insert(current), current);
        assert_eq!(state.focus_order(), vec![ShellComponent::ExitDialog]);
        assert_eq!(
            state
                .route_input_at(
                    InputEvent::mouse_down(PointerButton::Left, (0, 0)),
                    Instant::now()
                )
                .command,
            ShellCommand::CancelExit
        );
    }
}

#[test]
fn entering_overlays_accept_input_immediately_and_keep_modal_focus_isolated() {
    let transition = |kind, progress| ui::MotionTransition {
        kind,
        direction: ui::MotionDirection::Entering,
        progress,
        phase_progress: progress,
        active: progress < 1_000,
        next_redraw_in: Duration::from_millis(16),
    };
    for popup in [false, true] {
        let mut state = ShellSession::new_for_home_mode(
            ShellLaunchConfig::default(),
            (120, 40),
            ShellHomeMode::User,
        );
        let (kind, owner) = if popup {
            state.pages.common.active_popup = Some(ShellPopup {
                owner: Some(ShellComponent::Home),
                anchor: (10, 10),
            });
            (
                ui::MotionTransitionKind::Popover,
                ShellComponent::ContextMenu,
            )
        } else {
            state.apply_input(InputEvent::from_key_label("Esc"));
            assert_eq!(
                state.active_overlay_descriptor().unwrap().category,
                ShellOverlayCategory::ShellModal
            );
            (ui::MotionTransitionKind::Dialog, ShellComponent::ExitDialog)
        };
        state.refresh_hit_map();
        let activate = state.route_key_input(&KeyInput::from_label("Enter"));
        assert_ne!(activate.1, ShellCommand::Noop);
        for progress in [0, 250, 499, 500, 1_000] {
            state.refresh_hit_map_with_motion(ui::MotionTransitions {
                overlay: Some(transition(kind, progress)),
                ..Default::default()
            });
            assert!(state.overlay_interaction_ready);
            assert_eq!(state.focus_order(), vec![owner]);
            assert_eq!(
                state.route_key_input(&KeyInput::from_label("Enter")),
                activate
            );
            assert_ne!(
                state.route_key_input(&KeyInput::from_label("Esc")).1,
                ShellCommand::Noop
            );
            assert_eq!(
                state
                    .route_input_at(
                        InputEvent::mouse_down(PointerButton::Left, (0, 0)),
                        Instant::now()
                    )
                    .command,
                if popup {
                    ShellCommand::RequestExit
                } else {
                    ShellCommand::CancelExit
                }
            );
        }
    }
}

#[test]
fn overlay_resolver_categories_share_immediate_input_focus_and_hit_regions() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    state.set_navigation_path(vec![ShellScreen::Clock]);
    state.pages.clock.clock_create_state = Some(ClockCreateState::default());
    assert_eq!(
        state
            .active_overlay_descriptor()
            .map(|overlay| overlay.category),
        Some(ShellOverlayCategory::PageDialog)
    );
    let entering = ui::MotionTransition {
        kind: ui::MotionTransitionKind::Dialog,
        direction: ui::MotionDirection::Entering,
        progress: 499,
        phase_progress: 499,
        active: true,
        next_redraw_in: Duration::from_millis(16),
    };
    state.refresh_hit_map_with_motion(ui::MotionTransitions {
        overlay: Some(entering),
        ..ui::MotionTransitions::default()
    });
    assert_eq!(
        state.route_key_input(&KeyInput::from_label("Enter")).1,
        ShellCommand::ClockCreateFormInput(InputEvent::from_key_label("Enter"))
    );
    assert_eq!(
        state.route_key_input(&KeyInput::from_label("Esc")).1,
        ShellCommand::ClockCreateFormInput(InputEvent::from_key_label("Esc"))
    );
    assert!(
        state
            .focus_order()
            .contains(&ShellComponent::ClockCreateInput)
    );

    state.apply_input(InputEvent::key(InputKey::Char('1')));
    assert_eq!(
        state
            .pages
            .clock
            .clock_create_state
            .as_ref()
            .unwrap()
            .values[0],
        1
    );

    let mut state = explorer_routing_test_state();
    state.pages.explorer.explorer_overlay_mode = Some(ExplorerOverlayMode::Options);
    assert_eq!(
        state
            .active_overlay_descriptor()
            .map(|overlay| overlay.category),
        Some(ShellOverlayCategory::PagePopover)
    );
    state.pages.common.active_popup = Some(ShellPopup {
        owner: Some(ShellComponent::Explorer),
        anchor: (10, 10),
    });
    assert_eq!(
        state
            .active_overlay_descriptor()
            .map(|overlay| overlay.category),
        Some(ShellOverlayCategory::PagePopover)
    );

    state.pages.common.active_popup = None;
    state.pages.explorer.explorer_overlay_mode = None;
    state.set_navigation_path(vec![ShellScreen::Home]);
    state.notify_toast("Saved");
    let toast = state.active_overlay_descriptor().expect("toast descriptor");
    assert_eq!(toast.category, ShellOverlayCategory::Toast);
    assert!(toast.target.is_none());
    state.refresh_hit_map_with_motion(ui::MotionTransitions {
        overlay: Some(ui::MotionTransition {
            kind: ui::MotionTransitionKind::Toast,
            ..entering
        }),
        ..ui::MotionTransitions::default()
    });
    assert!(state.overlay_interaction_ready);
    assert_ne!(
        state.route_key_input(&KeyInput::from_label("Enter")).1,
        ShellCommand::Noop
    );
}

#[test]
fn setup_clock_and_diagnostics_keep_exact_overlay_surfaces_during_entry() {
    let entering = ui::MotionTransitions {
        overlay: Some(ui::MotionTransition {
            kind: ui::MotionTransitionKind::Dialog,
            direction: ui::MotionDirection::Entering,
            progress: 100,
            phase_progress: 100,
            active: true,
            next_redraw_in: Duration::from_millis(16),
        }),
        ..ui::MotionTransitions::default()
    };
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    let terminal = Rect::new(0, 0, 120, 40);
    let ui::ShellLayout::Full { main, .. } = ui::compute_shell_layout(terminal) else {
        panic!("expected full layout");
    };

    state.set_navigation_path(vec![ShellScreen::FirstRunSetup]);
    state.pages.setup.setup_custom_color_target = Some(ui::SetupCustomColorTarget::Theme);
    let setup_area = ui::setup_custom_color_dialog_area(main);
    for motion in [ui::MotionTransitions::default(), entering] {
        state.refresh_hit_map_with_motion(motion);
        assert_eq!(
            overlay_areas_for(&state, ShellComponent::SetupCustomColorDialog),
            vec![setup_area]
        );
    }

    state.pages.setup.setup_custom_color_target = None;
    state.set_navigation_path(vec![ShellScreen::Clock]);
    state.pages.clock.clock_create_state = Some(ClockCreateState::default());
    let clock_model = state.to_clock_view_model();
    let clock_layout = ui::clock_page_layout(main, &clock_model)
        .create_dialog
        .expect("create dialog");
    state.refresh_hit_map();
    assert_eq!(
        overlay_areas_for(&state, ShellComponent::ClockCreateDialog),
        vec![clock_layout.dialog]
    );
    assert!(
        state
            .focus_order()
            .contains(&ShellComponent::ClockCreateInput)
    );
    assert_eq!(
        state.route_key_input(&KeyInput::from_label("Tab")).1,
        ShellCommand::ClockCreateFormInput(InputEvent::from_key_label("Tab"))
    );
    state.refresh_hit_map_with_motion(entering);
    assert_eq!(
        overlay_areas_for(&state, ShellComponent::ClockCreateDialog),
        vec![clock_layout.dialog]
    );
    assert!(
        state
            .focus_order()
            .contains(&ShellComponent::ClockCreateInput)
    );

    assert!(state.overlay_interaction_ready);
    state.apply_input(InputEvent::key(InputKey::Char('1')));
    assert_eq!(
        state
            .pages
            .clock
            .clock_create_state
            .as_ref()
            .unwrap()
            .values[0],
        1
    );

    state.pages.clock.clock_create_state = None;
    state.set_navigation_path(vec![ShellScreen::Diagnostics]);
    state.pages.diagnostics.diagnostics_repair_preview =
        vec![app::diagnostics::DiagnosticsRepairAction::CreateDirectory {
            label: "Data".into(),
            path: std::path::PathBuf::from("/private/example/data"),
        }];
    let diagnostics_model = state.to_diagnostics_view_model();
    let diagnostics_area = ui::diagnostics_layout(main, &diagnostics_model)
        .repair_dialog
        .expect("repair dialog")
        .dialog;
    for motion in [ui::MotionTransitions::default(), entering] {
        state.refresh_hit_map_with_motion(motion);
        assert_eq!(
            overlay_areas_for(&state, ShellComponent::DiagnosticsRepairDialog),
            vec![diagnostics_area]
        );
    }

    let repair_preview = state.pages.diagnostics.diagnostics_repair_preview.clone();
    state.set_navigation_path(vec![ShellScreen::SystemStatus]);
    state.pages.diagnostics.diagnostics_repair_preview = repair_preview;
    let system_dialog = state
        .to_diagnostics_view_model()
        .repair_dialog
        .expect("system-status repair dialog");
    let system_area = ui::diagnostics_repair_dialog_layout(main, &system_dialog).dialog;
    for motion in [ui::MotionTransitions::default(), entering] {
        state.refresh_hit_map_with_motion(motion);
        assert_eq!(
            overlay_areas_for(&state, ShellComponent::DiagnosticsRepairDialog),
            vec![system_area]
        );
    }
}

#[test]
fn rendered_overlay_resolver_ids_are_variant_stable_and_precedence_is_preserved() {
    let mut launcher = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    launcher.set_navigation_path(vec![ShellScreen::Launcher]);
    launcher.pages.launcher.launcher_pending_confirmation =
        Some(LauncherPendingConfirmation::Launch {
            id: "app-1".into(),
            path: "/first/display/path".into(),
            kind: LauncherExecutableKind::NativeBinary,
            new_session: false,
        });
    let launch = launcher
        .active_overlay_descriptor()
        .expect("launch confirm");
    assert_eq!(launch.kind, ui::MotionOverlayKind::Dialog);
    assert_eq!(launch.category, ShellOverlayCategory::PageDialog);
    assert_eq!(
        launch.target,
        Some(RoutedTarget::Component(ShellComponent::Launcher))
    );
    launcher.pages.launcher.launcher_pending_confirmation =
        Some(LauncherPendingConfirmation::Launch {
            id: "app-1".into(),
            path: "/changed/display/path".into(),
            kind: LauncherExecutableKind::NativeBinary,
            new_session: false,
        });
    assert_eq!(
        launcher
            .active_overlay_descriptor()
            .expect("stable launch")
            .id,
        launch.id
    );

    launcher.pages.launcher.launcher_pending_confirmation =
        Some(LauncherPendingConfirmation::Remove {
            ids: vec!["app-1".into()],
            label: "First label".into(),
        });
    let remove = launcher
        .active_overlay_descriptor()
        .expect("remove confirm");
    launcher.pages.launcher.launcher_pending_confirmation =
        Some(LauncherPendingConfirmation::Remove {
            ids: vec!["app-1".into()],
            label: "Changed label".into(),
        });
    assert_eq!(
        launcher
            .active_overlay_descriptor()
            .expect("stable remove")
            .id,
        remove.id
    );
    assert_ne!(launch.id, remove.id);

    launcher.pages.common.active_popup = Some(ShellPopup {
        owner: Some(ShellComponent::Launcher),
        anchor: (2, 2),
    });
    assert_eq!(
        launcher
            .active_overlay_descriptor()
            .expect("popup precedence")
            .category,
        ShellOverlayCategory::ContextPopup
    );
    launcher.notify_modal(
        "Priority",
        "Shell modal wins",
        ui::NotificationTone::Info,
        vec![ShellNotificationAction::new("ok", "OK")],
    );
    assert_eq!(
        launcher
            .active_overlay_descriptor()
            .expect("modal precedence")
            .category,
        ShellOverlayCategory::ShellModal
    );

    let mut editor = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    editor.set_navigation_path(vec![ShellScreen::Editor]);
    let mut menu_ids = Vec::new();
    for menu in [
        ui::EditorMenu::File,
        ui::EditorMenu::Edit,
        ui::EditorMenu::Insert,
        ui::EditorMenu::Format,
        ui::EditorMenu::View,
        ui::EditorMenu::Settings,
    ] {
        editor.pages.editor.editor_open_menu = Some(menu);
        let overlay = editor.active_overlay_descriptor().expect("editor menu");
        assert_eq!(overlay.kind, ui::MotionOverlayKind::Popover);
        assert_eq!(overlay.category, ShellOverlayCategory::PagePopover);
        assert_eq!(
            overlay.target,
            Some(RoutedTarget::Component(ShellComponent::Editor))
        );
        menu_ids.push(overlay.id);
    }
    menu_ids.sort();
    menu_ids.dedup();
    assert_eq!(menu_ids.len(), 6);
    editor.pages.editor.editor_open_menu = None;
    editor.pages.editor.editor_quick_menu_anchor = Some((4, 5));
    let quick = editor.active_overlay_descriptor().expect("quick menu");
    editor.pages.editor.editor_quick_menu_anchor = Some((40, 15));
    assert_eq!(
        editor
            .active_overlay_descriptor()
            .expect("moved quick menu")
            .id,
        quick.id
    );
}

#[test]
fn newly_tracked_overlay_groups_keep_keyboard_mouse_focus_ready_during_entry() {
    let (mut launcher, platform, _guard) = explorer_terminal_test_fixture();
    launcher.set_navigation_path(vec![ShellScreen::Launcher]);
    launcher.pages.launcher.launcher_pending_confirmation =
        Some(LauncherPendingConfirmation::Launch {
            id: "app-1".into(),
            path: "/app".into(),
            kind: LauncherExecutableKind::NativeBinary,
            new_session: false,
        });

    let mut editor = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    editor.set_navigation_path(vec![ShellScreen::Editor]);
    editor.app.dispatch_at(
        app::AppCommand::SetEditorState(Some(EditorState::untitled(
            app::editor::DocumentKind::PlainText,
        ))),
        Instant::now(),
    );
    editor.pages.editor.editor_open_menu = Some(ui::EditorMenu::File);

    for (state, kind, owner) in [
        (
            &mut launcher,
            ui::MotionTransitionKind::Dialog,
            ShellComponent::Launcher,
        ),
        (
            &mut editor,
            ui::MotionTransitionKind::Popover,
            ShellComponent::Editor,
        ),
    ] {
        state.refresh_hit_map();
        let activate = state.route_key_input(&KeyInput::from_label("Enter"));
        assert_ne!(activate.1, ShellCommand::Noop);
        let entering = ui::MotionTransition {
            kind,
            direction: ui::MotionDirection::Entering,
            progress: 0,
            phase_progress: 0,
            active: true,
            next_redraw_in: Duration::from_millis(16),
        };
        state.refresh_hit_map_with_motion(ui::MotionTransitions {
            overlay: Some(entering),
            ..ui::MotionTransitions::default()
        });
        assert!(state.overlay_interaction_ready);
        assert_eq!(
            state.route_key_input(&KeyInput::from_label("Enter")),
            activate
        );
        assert_ne!(
            state.route_key_input(&KeyInput::from_label("Esc")).1,
            ShellCommand::Noop
        );
        assert_eq!(state.focus_order(), vec![owner]);
        let has_surface =
            state.hit_map().regions().iter().any(|region| {
                region.layer == ShellHitLayer::AppOverlay && region.component == owner
            });
        assert!(has_surface, "missing overlay surface for {owner:?}");
        state.refresh_hit_map_with_motion(ui::MotionTransitions {
            overlay: Some(ui::MotionTransition {
                progress: 1_000,
                phase_progress: 1_000,
                active: false,
                next_redraw_in: Duration::ZERO,
                ..entering
            }),
            ..ui::MotionTransitions::default()
        });
        assert!(state.overlay_interaction_ready);
        assert_ne!(
            state.route_key_input(&KeyInput::from_label("Enter")).1,
            ShellCommand::Noop
        );

        state.refresh_hit_map_with_motion(ui::MotionTransitions::default());
        assert!(
            state.overlay_interaction_ready,
            "Reduced/inactive motion settles ready"
        );

        state.refresh_hit_map_with_motion(ui::MotionTransitions {
            overlay: Some(entering),
            ..ui::MotionTransitions::default()
        });
        let launcher_before = state.pages.launcher.clone();
        let editor_before = state.app.editor_state().cloned();
        let overlay_before = state.active_overlay_descriptor();
        let calls_before = platform.calls();
        let now = Instant::now();
        state.apply_input_with_platform_at(
            InputEvent::mouse_down(PointerButton::Left, (1, 1)),
            &platform,
            now,
        );
        state.apply_input_with_platform_at(
            InputEvent::mouse_up(PointerButton::Left, (1, 1)),
            &platform,
            now + Duration::from_millis(20),
        );
        assert_eq!(state.pages.launcher, launcher_before);
        assert_eq!(state.app.editor_state(), editor_before.as_ref());
        assert_eq!(platform.calls(), calls_before);
        if owner != ShellComponent::Editor {
            assert_eq!(state.active_overlay_descriptor(), overlay_before);
        }
    }
}

#[test]
fn rendered_editor_menu_hit_surface_matches_editor_layout() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    state.set_navigation_path(vec![ShellScreen::Editor]);
    state.app.dispatch_at(
        app::AppCommand::SetEditorState(Some(EditorState::untitled(
            app::editor::DocumentKind::PlainText,
        ))),
        Instant::now(),
    );
    state.pages.editor.editor_open_menu = Some(ui::EditorMenu::File);
    state.refresh_hit_map();
    let ui::ShellLayout::Full { main, .. } = ui::compute_shell_layout(Rect::new(0, 0, 120, 40))
    else {
        panic!("full layout");
    };
    let expected = ui::editor_layout(main, &state.to_editor_view_model())
        .menu_popup
        .expect("rendered menu popup");
    let surfaces = state
        .hit_map()
        .regions()
        .iter()
        .filter(|region| {
            region.layer == ShellHitLayer::AppOverlay && region.component == ShellComponent::Editor
        })
        .map(|region| region.area)
        .collect::<Vec<_>>();
    assert_eq!(surfaces, vec![expected]);
}

#[test]
fn settings_editor_identities_distinguish_variants_without_mutable_content() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    state.enter_screen(ShellScreen::Settings);
    state.pages.settings.settings_state = Some(SettingsState {
        focus: Default::default(),
        form_selected: 0,
        form_interaction: Default::default(),
        application: Default::default(),
        system_time: Default::default(),
        animation_editor: None,
        number_editor: None,
        category: ui::SettingsCategory::Appearance,
        selected_field: ui::SettingsField::Theme,
        status: String::new().into(),
        scroll_offset: 0,
        picker: None,
        color_editor: None,
        weather_location_editor: None,
        file_extensions_editor: None,
        time_sync_server_editor: None,
        time_sync_validation_request_id: None,
    });

    let mut ids = Vec::new();
    for kind in [
        ui::SettingsPickerKind::BorderColor,
        ui::SettingsPickerKind::AccentColor,
    ] {
        let settings = state
            .pages
            .settings
            .settings_state
            .as_mut()
            .expect("settings");
        settings.color_editor = Some(SettingsColorEditorState {
            editing: Default::default(),
            kind,
            value: "#112233".into(),
            error: Some("first error".into()),
        });
        let first = state.active_overlay_descriptor().expect("color editor");
        let settings = state
            .pages
            .settings
            .settings_state
            .as_mut()
            .expect("settings");
        let editor = settings.color_editor.as_mut().expect("color editor");
        editor.value = "#abcdef".into();
        editor.error = Some("changed error".into());
        assert_eq!(
            state.active_overlay_descriptor().expect("stable color").id,
            first.id
        );
        ids.push(first.id);
        state
            .pages
            .settings
            .settings_state
            .as_mut()
            .expect("settings")
            .color_editor = None;
    }

    state
        .pages
        .settings
        .settings_state
        .as_mut()
        .expect("settings")
        .weather_location_editor = Some(SettingsWeatherLocationEditorState {
        editing: Default::default(),
        value: "first".into(),
        error: None,
    });
    ids.push(
        state
            .active_overlay_descriptor()
            .expect("weather editor")
            .id,
    );
    state
        .pages
        .settings
        .settings_state
        .as_mut()
        .expect("settings")
        .weather_location_editor = None;
    state
        .pages
        .settings
        .settings_state
        .as_mut()
        .expect("settings")
        .file_extensions_editor = Some(SettingsFileExtensionsEditorState {
        editing: Default::default(),
        value: "md".into(),
        error: None,
    });
    ids.push(
        state
            .active_overlay_descriptor()
            .expect("extensions editor")
            .id,
    );
    state
        .pages
        .settings
        .settings_state
        .as_mut()
        .expect("settings")
        .file_extensions_editor = None;
    state
        .pages
        .settings
        .settings_state
        .as_mut()
        .expect("settings")
        .time_sync_server_editor = Some(SettingsTimeSyncServerEditorState {
        editing: Default::default(),
        value: "time.example".into(),
        error: Some("error".into()),
        validating: true,
    });
    let time_sync = state.active_overlay_descriptor().expect("time sync editor");
    let editor = state
        .pages
        .settings
        .settings_state
        .as_mut()
        .expect("settings")
        .time_sync_server_editor
        .as_mut()
        .expect("time sync editor");
    editor.value = "changed.example".into();
    editor.error = None;
    editor.validating = false;
    assert_eq!(
        state
            .active_overlay_descriptor()
            .expect("stable time sync")
            .id,
        time_sync.id
    );
    ids.push(time_sync.id);
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 5);
}

#[test]
fn exit_confirmation_distinguishes_program_and_computer_actions() {
    let root = std::env::temp_dir().join(format!(
        "tundra-shell-poweroff-label-{}",
        std::process::id()
    ));
    let user_dirs = platform::UserDirs::new(
        root.join("Desktop"),
        root.join("Documents"),
        root.join("Downloads"),
        root.join("Pictures"),
        root.join("Videos"),
        root.join("Music"),
        root.join("AppData"),
    )
    .expect("absolute mock user directories");
    let app_paths = platform::build_windows_app_paths(
        root.join("Roaming"),
        root.join("Local"),
        root.join("Temp"),
    )
    .expect("absolute mock app paths");
    let platform = platform::mock::MockPlatform::new(user_dirs, app_paths);
    let mut state = ShellSession::new(ShellLaunchConfig::default(), (120, 40));

    state.apply_input_with_platform(InputEvent::from_key_label("q"), &platform);

    let modal = state
        .to_notification_view_model()
        .expect("exit confirmation modal");
    let poweroff = modal
        .actions
        .iter()
        .find(|action| action.id == "poweroff")
        .expect("poweroff action");
    assert_eq!(poweroff.label, "Shut down computer");
    assert!(modal.stacked_actions);
    assert_eq!(
        modal
            .actions
            .iter()
            .map(|action| action.label.as_str())
            .collect::<Vec<_>>(),
        [
            "Exit TundraUX",
            "Restart TundraUX",
            "Restart computer",
            "Shut down computer",
            "Cancel"
        ]
    );

    for (key, expected, restart) in [
        ("b", ShellAction::Reboot, false),
        ("B", ShellAction::Reboot, false),
        ("p", ShellAction::PowerOff, false),
        ("r", ShellAction::Exit, true),
        ("y", ShellAction::Exit, false),
        ("Esc", ShellAction::Redraw, false),
    ] {
        let mut state = ShellSession::new(ShellLaunchConfig::default(), (120, 40));
        state.apply_input_with_platform(InputEvent::from_key_label("q"), &platform);
        assert_eq!(
            state.apply_input_with_platform(InputEvent::from_key_label(key), &platform),
            expected,
            "{key}"
        );
        assert_eq!(state.restart_requested(), restart, "{key}");
    }

    state.set_navigation_path(vec![
        ShellScreen::Home,
        ShellScreen::Editor,
        ShellScreen::ExitConfirm,
    ]);
    state.show_exit_confirmation_modal(&platform);
    state.pages.editor.editor_save_state = Some(EditorSaveState {
        id: 5,
        path: PathBuf::from("large.txt"),
        document_generation: state.pages.editor.editor_document_generation,
        revision: 1,
        stage: EditorTaskStage::Writing,
    });
    assert_eq!(
        state.apply_input_with_platform(InputEvent::from_key_label("b"), &platform),
        ShellAction::Redraw
    );
    assert!(
        state.to_notification_view_model().is_some(),
        "keep the menu available while saving"
    );
    state.pages.editor.editor_save_state = None;
    assert_eq!(
        state.apply_input_with_platform(InputEvent::from_key_label("b"), &platform),
        ShellAction::Reboot
    );

    let unsupported = platform.with_capabilities(platform::PlatformCapabilities::unsupported());
    let mut state = ShellSession::new(ShellLaunchConfig::default(), (120, 40));
    state.apply_input_with_platform(InputEvent::from_key_label("q"), &unsupported);
    let modal = state.to_notification_view_model().unwrap();
    assert!(
        modal
            .actions
            .iter()
            .all(|action| action.id != "reboot" && action.id != "poweroff")
    );
    assert_eq!(
        state.apply_input_with_platform(InputEvent::from_key_label("b"), &unsupported),
        ShellAction::Redraw
    );
}

#[test]
fn notification_alerts_resolve_by_key_and_preserve_other_sources() {
    let mut notifications = NotificationCenter::new("Ready");
    notifications.notify_alert_with_key(
        "settings",
        "Settings warning",
        ui::NotificationTone::Warning,
    );
    notifications.notify_alert_with_key(
        "explorer.operation",
        "Explorer failed",
        ui::NotificationTone::Error,
    );

    assert_eq!(notifications.alert().as_deref(), Some("Explorer failed"));
    assert_eq!(
        notifications.alert_tone(),
        Some(ui::NotificationTone::Error)
    );

    notifications.resolve_alert("explorer.operation");
    assert_eq!(notifications.alert().as_deref(), Some("Settings warning"));
    assert_eq!(
        notifications.alert_tone(),
        Some(ui::NotificationTone::Warning)
    );
}

#[test]
fn keyed_modal_update_replaces_presentation_binding_atomically() {
    let mut state = ShellSession::new(ShellLaunchConfig::default(), (80, 24));
    let first_id = state.notify_modal_with_options(
        ShellNotification::modal(
            "First",
            "Original",
            ui::NotificationTone::Info,
            vec![
                ShellNotificationAction::new("continue", "Continue")
                    .with_follow_up(ShellCommand::FocusNext),
            ],
        )
        .with_key("same"),
    );
    let updated_id = state.notify_modal_with_options(
        ShellNotification::modal(
            "Updated",
            "Replacement",
            ui::NotificationTone::Warning,
            vec![
                ShellNotificationAction::new("continue", "Continue")
                    .with_follow_up(ShellCommand::FocusPrevious),
            ],
        )
        .with_key("same"),
    );

    assert_eq!(updated_id, first_id);
    assert_eq!(
        state
            .to_notification_view_model()
            .map(|model| (model.title, model.message)),
        Some(("Updated".to_string(), "Replacement".to_string()))
    );
    state.activate_notification_selected();
    assert_eq!(
        state.pending_notification_commands.pop_front(),
        Some(ShellCommand::PageAction(
            state.active_page_id(),
            Box::new(ShellCommand::FocusPrevious)
        ))
    );
}

#[test]
fn notification_follow_up_activation_is_iterative_and_bounded() {
    let mut state = ShellSession::new(ShellLaunchConfig::default(), (80, 24));
    for index in 0..(MAX_NOTIFICATION_FOLLOW_UP_STEPS + 3) {
        state.notify_modal(
            format!("Notice {index}"),
            "Continue?",
            ui::NotificationTone::Info,
            vec![
                ShellNotificationAction::new(format!("ok-{index}"), "OK")
                    .with_follow_up(ShellCommand::NotificationActivateSelected),
            ],
        );
    }

    let action = state.apply_input(InputEvent::from_key_label("Enter"));

    assert_eq!(action, ShellAction::Redraw);
    assert!(state.to_notification_view_model().is_some());
    assert_eq!(
        state.to_shell_chrome_view_model().status.error.as_deref(),
        Some("Notification follow-up limit reached")
    );
    assert_eq!(
        state.to_shell_chrome_view_model().status.alert_tone,
        ui::NotificationTone::Critical
    );
}

#[test]
fn watchdog_incident_shows_details_and_view_actions_to_standard_users() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    state.app.dispatch_at(
        app::AppCommand::SetAuthSession(Some(AuthSession {
            source: identity::IdentitySource::SystemCurrentProcess,
            session_id: "user-session".to_string(),
            user_id: "user-id".to_string(),
            username: "user".to_string(),
            role: UserRole::User,
            started_at_epoch_ms: 1,
        })),
        Instant::now(),
    );
    show_watchdog_incident(
        &mut state,
        IncidentReceipt {
            incident_id: "SECRET-INCIDENT-ID".to_string(),
            kind: watchdog::IncidentKind::Error,
            severity: watchdog::IncidentSeverity::Critical,
            app_id: None,
            component: Some("private-component".to_string()),
            task_id: None,
            task_group: None,
            boundary: "private-boundary".to_string(),
            panic_action: None,
            operation_kind: None,
            operation_id: None,
            recovery_handler_version: None,
            restart_attempt: 0,
            summary: "SECRET watchdog summary".to_string(),
            source_location: None,
            recovery: RecoveryOutcome::Recovered("SECRET recovery detail".to_string()),
            json_report_path: Some(std::path::PathBuf::from(
                "/private/reports/SECRET-INCIDENT-ID.json",
            )),
            text_report_path: None,
        },
    );

    let modal = state.to_notification_view_model().expect("watchdog modal");
    assert!(modal.message.contains("SECRET"));
    assert!(modal.message.contains("/private"));
    assert!(
        modal
            .actions
            .iter()
            .any(|action| action.id == "open-report")
    );
    assert!(
        modal
            .actions
            .iter()
            .any(|action| action.id == "copy-summary")
    );
}

#[test]
fn previous_unclean_exit_does_not_interrupt_setup() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    state.set_navigation_path(vec![ShellScreen::FirstRunSetup]);
    state.pages.common.focused_component = ShellComponent::SetupLanguage;
    let report_path = std::path::PathBuf::from("/reports/previous-run.txt");

    show_watchdog_incident(
        &mut state,
        IncidentReceipt {
            incident_id: "unclean-previous-run".to_string(),
            kind: IncidentKind::UncleanExit,
            severity: watchdog::IncidentSeverity::Critical,
            app_id: None,
            component: None,
            task_id: None,
            task_group: None,
            boundary: "process.unhandled".to_string(),
            panic_action: None,
            operation_kind: None,
            operation_id: None,
            recovery_handler_version: None,
            restart_attempt: 0,
            summary: "previous run ended without a clean shutdown".to_string(),
            source_location: None,
            recovery: RecoveryOutcome::Unrecoverable(
                "the previous process had already terminated".to_string(),
            ),
            json_report_path: None,
            text_report_path: Some(report_path.clone()),
        },
    );

    assert_eq!(state.active_screen(), ShellScreen::FirstRunSetup);
    assert_eq!(state.focused_component(), ShellComponent::SetupLanguage);
    assert!(state.to_notification_view_model().is_none());
    assert_eq!(state.latest_watchdog_report.as_ref(), Some(&report_path));
    assert!(
        state
            .latest_watchdog_summary
            .as_deref()
            .is_some_and(|summary| summary.contains("previous run ended"))
    );
}
