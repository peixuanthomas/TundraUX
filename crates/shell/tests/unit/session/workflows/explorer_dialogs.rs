use super::*;

fn explorer_overlay_surface(state: &ShellSession) -> Rect {
    let terminal = Rect::new(0, 0, state.terminal_size.0, state.terminal_size.1);
    let ui::ShellLayout::Full { main, .. } = ui::compute_shell_layout(terminal) else {
        panic!("expected full shell layout");
    };
    ui::explorer_layout(main, &state.to_explorer_view_model())
        .overlay
        .expect("active Explorer overlay layout")
        .area
}

#[test]
fn explorer_dialog_and_transient_overlay_identities_are_semantic_and_stable() {
    let cases = [
        (
            ResolvedExplorerOverlay::RestoreConflict,
            "explorer-restore-conflict",
        ),
        (
            ResolvedExplorerOverlay::OperationConflict,
            "explorer-operation-conflict",
        ),
        (
            ResolvedExplorerOverlay::Input(ExplorerInputMode::NewFolder),
            "explorer-input:new-folder",
        ),
        (
            ResolvedExplorerOverlay::Input(ExplorerInputMode::NewTextFile),
            "explorer-input:new-text-file",
        ),
        (
            ResolvedExplorerOverlay::Input(ExplorerInputMode::Rename),
            "explorer-input:rename",
        ),
        (
            ResolvedExplorerOverlay::Input(ExplorerInputMode::RestoreDestination),
            "explorer-input:restore-destination",
        ),
    ];
    for (resolved, expected) in cases {
        let descriptor = resolved.descriptor();
        assert_eq!(descriptor.id, expected);
        assert_eq!(descriptor.kind, ui::MotionOverlayKind::Dialog);
    }

    let mut state = explorer_routing_test_state();
    state.pages.explorer.explorer_input_mode = ExplorerInputMode::NewFolder;
    state.pages.explorer.explorer_input = "first typed value".into();
    let input = state
        .active_overlay_descriptor()
        .expect("new folder dialog");
    assert_eq!(input.kind, ui::MotionOverlayKind::Dialog);
    assert_eq!(input.category, ShellOverlayCategory::PageDialog);
    assert_eq!(
        input.target,
        Some(RoutedTarget::Component(ShellComponent::Explorer))
    );
    state.pages.explorer.explorer_input = "changed typed value".into();
    assert_eq!(
        state.active_overlay_descriptor().expect("stable input").id,
        input.id
    );
    state.pages.common.active_popup = Some(ShellPopup {
        owner: Some(ShellComponent::Explorer),
        anchor: (1, 1),
    });
    assert!(matches!(
        state.resolved_explorer_overlay(),
        Some(ResolvedExplorerOverlay::Input(ExplorerInputMode::NewFolder))
    ));
    assert_eq!(
        state.route_key_input(&KeyInput::from_label("Enter")).1,
        ShellCommand::ExplorerEditInput(InputEvent::from_key_label("Enter"))
    );
    assert_eq!(
        state.route_key_input(&KeyInput::from_label("Esc")).1,
        ShellCommand::ExplorerEditInput(InputEvent::from_key_label("Esc"))
    );

    state.pages.explorer.explorer_input_mode = ExplorerInputMode::Browse;
    state.pages.common.active_popup = None;
    let mut explorer = state.app.explorer_state().expect("explorer state").clone();
    explorer.pending_conflict = Some(app::explorer::ExplorerConflict {
        source: "/source-a".into(),
        target: "/target-a".into(),
        remaining: 3,
    });
    state.app.dispatch_at(
        app::AppCommand::SetExplorerState(Some(explorer.clone())),
        Instant::now(),
    );
    let conflict = state
        .active_overlay_descriptor()
        .expect("operation conflict");
    explorer.pending_conflict = Some(app::explorer::ExplorerConflict {
        source: "/source-b".into(),
        target: "/target-b".into(),
        remaining: 1,
    });
    state.app.dispatch_at(
        app::AppCommand::SetExplorerState(Some(explorer)),
        Instant::now(),
    );
    assert_eq!(
        state
            .active_overlay_descriptor()
            .expect("stable conflict")
            .id,
        conflict.id
    );
    state.pages.common.active_popup = Some(ShellPopup {
        owner: Some(ShellComponent::Explorer),
        anchor: (1, 1),
    });
    assert_eq!(
        state.resolved_explorer_overlay(),
        Some(ResolvedExplorerOverlay::OperationConflict)
    );
    assert_eq!(
        state.route_key_input(&KeyInput::from_label("Enter")).1,
        ShellCommand::ExplorerOverlayActivate
    );
    assert_eq!(
        state.route_key_input(&KeyInput::from_label("r")).1,
        ShellCommand::ExplorerConflictReplace
    );
    assert_eq!(
        state.route_key_input(&KeyInput::from_label("s")).1,
        ShellCommand::ExplorerConflictSkip
    );
    assert_eq!(
        state.route_key_input(&KeyInput::from_label("a")).1,
        ShellCommand::ExplorerConflictToggleApplyToRemaining
    );
    assert_eq!(
        state.route_key_input(&KeyInput::from_label("Esc")).1,
        ShellCommand::ExplorerConflictCancel
    );

    state.pages.common.active_popup = Some(ShellPopup {
        owner: Some(ShellComponent::Explorer),
        anchor: (1, 1),
    });
    let popup = state.active_overlay_descriptor().expect("popup");
    state.pages.common.active_popup = Some(ShellPopup {
        owner: Some(ShellComponent::Explorer),
        anchor: (80, 20),
    });
    assert_eq!(
        state.active_overlay_descriptor().expect("moved popup").id,
        popup.id
    );

    state.pages.common.active_popup = None;
    let mut explorer = state.app.explorer_state().expect("explorer state").clone();
    explorer.pending_conflict = None;
    state.app.dispatch_at(
        app::AppCommand::SetExplorerState(Some(explorer)),
        Instant::now(),
    );
    state.pages.common.active_popup = Some(ShellPopup {
        owner: Some(ShellComponent::Explorer),
        anchor: (5, 5),
    });
    for (mode, expected) in [
        (
            ExplorerOverlayMode::ContextMenu { anchor: (1, 1) },
            "explorer-popover:context-menu",
        ),
        (
            ExplorerOverlayMode::Sort { anchor: (2, 2) },
            "explorer-popover:sort",
        ),
        (ExplorerOverlayMode::Options, "explorer-popover:options"),
        (
            ExplorerOverlayMode::Properties,
            "explorer-popover:properties",
        ),
    ] {
        state.pages.explorer.explorer_overlay_mode = Some(mode);
        assert_eq!(
            state
                .active_overlay_descriptor()
                .expect("explorer popover")
                .id,
            expected
        );
        assert_eq!(
            state
                .active_overlay_descriptor()
                .expect("semantic popover")
                .category,
            ShellOverlayCategory::PagePopover
        );
    }
    state.pages.explorer.explorer_overlay_mode =
        Some(ExplorerOverlayMode::ContextMenu { anchor: (90, 30) });
    assert_eq!(
        state
            .active_overlay_descriptor()
            .expect("moved context menu")
            .id,
        "explorer-popover:context-menu"
    );

    let mut explorer = state.app.explorer_state().expect("explorer state").clone();
    explorer.pending_dialog = Some(app::explorer::ExplorerDialog::delete(std::path::Path::new(
        "/first",
    )));
    state.app.dispatch_at(
        app::AppCommand::SetExplorerState(Some(explorer.clone())),
        Instant::now(),
    );
    assert_eq!(
        state
            .active_overlay_descriptor()
            .expect("popover hides dialog")
            .id,
        "explorer-popover:context-menu"
    );
    state.pages.explorer.explorer_overlay_mode = None;
    let delete = state.active_overlay_descriptor().expect("delete dialog");
    assert_eq!(delete.id, "explorer-dialog:delete-to-trash");
    assert_eq!(delete.kind, ui::MotionOverlayKind::Dialog);
    explorer.pending_dialog = Some(app::explorer::ExplorerDialog::dump_trash(4));
    state.app.dispatch_at(
        app::AppCommand::SetExplorerState(Some(explorer)),
        Instant::now(),
    );
    assert_eq!(
        state.active_overlay_descriptor().expect("dump dialog").id,
        "explorer-dialog:dump-trash"
    );

    let mut toast = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    let instance = Instant::now();
    toast.app.dispatch_at(
        app::AppCommand::Notification(app::NotificationCommand::ShowToast("First".into())),
        instance,
    );
    let first = toast.active_overlay_descriptor().expect("toast instance");
    toast.app.dispatch_at(
        app::AppCommand::Notification(app::NotificationCommand::ShowToast("Changed".into())),
        instance,
    );
    assert_eq!(
        toast.active_overlay_descriptor().expect("same instance").id,
        first.id
    );
    toast.app.dispatch_at(
        app::AppCommand::Notification(app::NotificationCommand::ShowToast("Changed".into())),
        instance + Duration::from_secs(1),
    );
    assert_ne!(
        toast.active_overlay_descriptor().expect("renewed toast").id,
        first.id
    );
    assert!(
        toast
            .active_overlay_descriptor()
            .expect("toast target")
            .target
            .is_none()
    );
}

#[test]
fn explorer_address_and_search_keep_non_overlay_text_input_routing() {
    for mode in [ExplorerInputMode::Address, ExplorerInputMode::Search] {
        let (mut state, platform, _fixture) = explorer_terminal_test_fixture();
        state.storage_manager = Some(
            StorageManager::open(platform.app_paths().unwrap())
                .unwrap()
                .manager,
        );
        state.begin_explorer_input(mode);
        state.pages.explorer.explorer_input = "界a".into();
        state.pages.explorer.explorer_address_cursor = 2;
        state.pages.explorer.explorer_address_selection_anchor = None;

        assert_eq!(state.resolved_explorer_overlay(), None);
        assert_eq!(state.active_overlay_descriptor(), None);
        assert_eq!(
            state.route_key_input(&KeyInput::from_label("Enter")).1,
            ShellCommand::SubmitExplorerInput
        );
        assert_eq!(
            state.route_key_input(&KeyInput::from_label("Esc")).1,
            ShellCommand::CancelExplorerInput
        );
        for (label, expected, cursor) in [
            ("Backspace", "界", 1),
            ("x", "界x", 2),
            ("Home", "界x", 0),
            ("x", "x界x", 1),
            ("End", "x界x", 3),
            ("Backspace", "x界", 2),
        ] {
            let key = KeyInput::from_label(label);
            assert_eq!(
                state.route_key_input(&key).1,
                ShellCommand::ExplorerEditInput(InputEvent::Key(key.clone()))
            );
            state.apply_input_with_platform(InputEvent::Key(key), &platform);
            assert_eq!(
                state.pages.explorer.explorer_input, expected,
                "{mode:?}: {label}"
            );
            assert_eq!(
                state.pages.explorer.explorer_address_cursor, cursor,
                "{mode:?}: {label}"
            );
            if mode == ExplorerInputMode::Search {
                assert_eq!(state.app.explorer_state().unwrap().query, expected);
            }
        }
        state.apply_input_with_platform(InputEvent::from_key_label("Esc"), &platform);
        assert_eq!(
            state.pages.explorer.explorer_input_mode,
            ExplorerInputMode::Browse
        );
        assert!(state.pages.explorer.explorer_input.is_empty());
    }
}

#[test]
fn explorer_dialogs_accept_input_during_entry_and_keep_their_hit_maps_isolated() {
    let entering = ui::MotionTransition {
        kind: ui::MotionTransitionKind::Dialog,
        direction: ui::MotionDirection::Entering,
        progress: 0,
        phase_progress: 0,
        active: true,
        next_redraw_in: Duration::from_millis(16),
    };
    for mode in [
        ExplorerInputMode::NewFolder,
        ExplorerInputMode::NewTextFile,
        ExplorerInputMode::Rename,
        ExplorerInputMode::RestoreDestination,
    ] {
        let (mut state, platform, _guard) = explorer_terminal_test_fixture();
        let covered = explorer_test_layout(&state).rows[0].area;
        state.pages.explorer.explorer_input_mode = mode;
        state.pages.common.active_popup = Some(ShellPopup {
            owner: Some(ShellComponent::Explorer),
            anchor: (1, 1),
        });
        assert_eq!(
            state.resolved_explorer_overlay(),
            Some(ResolvedExplorerOverlay::Input(mode))
        );
        assert_eq!(
            state.route_key_input(&KeyInput::from_label("Enter")).1,
            ShellCommand::ExplorerEditInput(InputEvent::from_key_label("Enter"))
        );
        assert_eq!(
            state.route_key_input(&KeyInput::from_label("Esc")).1,
            ShellCommand::ExplorerEditInput(InputEvent::from_key_label("Esc"))
        );
        state.refresh_hit_map_with_motion(ui::MotionTransitions {
            overlay: Some(entering),
            ..ui::MotionTransitions::default()
        });
        assert!(state.overlay_interaction_ready);
        assert_eq!(
            state.route_key_input(&KeyInput::from_label("Enter")).1,
            ShellCommand::ExplorerEditInput(InputEvent::from_key_label("Enter"))
        );
        assert_ne!(
            state.route_key_input(&KeyInput::from_label("Esc")).1,
            ShellCommand::Noop
        );
        assert_eq!(state.focus_order(), vec![ShellComponent::Explorer]);
        assert_eq!(
            state
                .hit_map()
                .regions()
                .iter()
                .filter(|region| region.layer == ShellHitLayer::AppOverlay)
                .map(|region| region.area)
                .collect::<Vec<_>>(),
            vec![explorer_overlay_surface(&state)]
        );
        let explorer_before = state.app.explorer_state().cloned();
        let overlay_before = state.active_overlay_descriptor();
        let calls_before = platform.calls();
        for point in [(1, 1), (covered.right() - 1, covered.y)] {
            assert!(!rect_contains(explorer_overlay_surface(&state), point));
            let now = Instant::now();
            state.apply_input_with_platform_at(
                InputEvent::mouse_down(PointerButton::Left, point),
                &platform,
                now,
            );
            state.apply_input_with_platform_at(
                InputEvent::mouse_up(PointerButton::Left, point),
                &platform,
                now + Duration::from_millis(20),
            );
            assert_eq!(state.app.explorer_state(), explorer_before.as_ref());
            assert_eq!(state.pages.explorer.explorer_input_mode, mode);
            assert_eq!(state.active_overlay_descriptor(), overlay_before);
            assert_eq!(platform.calls(), calls_before);
        }
        state.apply_input_with_platform(InputEvent::Paste("新名字".into()), &platform);
        assert_eq!(state.pages.explorer.explorer_input, "新名字");
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
        state.refresh_hit_map_with_motion(ui::MotionTransitions::default());
        assert!(state.overlay_interaction_ready);
    }

    let mut conflict = explorer_routing_test_state();
    let mut explorer = conflict
        .app
        .explorer_state()
        .expect("explorer state")
        .clone();
    explorer.pending_conflict = Some(app::explorer::ExplorerConflict {
        source: "/source".into(),
        target: "/target".into(),
        remaining: 1,
    });
    conflict.app.dispatch_at(
        app::AppCommand::SetExplorerState(Some(explorer)),
        Instant::now(),
    );
    conflict.refresh_hit_map();
    let activate = conflict.route_key_input(&KeyInput::from_label("Enter"));
    assert_ne!(activate.1, ShellCommand::Noop);
    conflict.refresh_hit_map_with_motion(ui::MotionTransitions {
        overlay: Some(entering),
        ..ui::MotionTransitions::default()
    });
    assert_eq!(
        conflict.route_key_input(&KeyInput::from_label("Enter")),
        activate
    );
    assert_ne!(
        conflict.route_key_input(&KeyInput::from_label("Esc")).1,
        ShellCommand::Noop
    );
}

#[test]
fn explorer_semantic_replacements_route_input_immediately_to_the_active_popup() {
    let replacing = |kind| ui::MotionTransition {
        kind,
        direction: ui::MotionDirection::Replacing,
        progress: 0,
        phase_progress: 0,
        active: true,
        next_redraw_in: Duration::from_millis(16),
    };
    for mode in [
        ExplorerOverlayMode::Sort { anchor: (3, 3) },
        ExplorerOverlayMode::Options,
        ExplorerOverlayMode::Properties,
    ] {
        let (mut state, platform, _guard) = explorer_terminal_test_fixture();
        let covered = explorer_test_layout(&state).rows[0].area;
        state.pages.common.active_popup = Some(ShellPopup {
            owner: Some(ShellComponent::Explorer),
            anchor: (1, 1),
        });
        state.pages.explorer.explorer_overlay_mode = Some(mode);
        assert!(matches!(
            state.resolved_explorer_overlay(),
            Some(ResolvedExplorerOverlay::Semantic(resolved)) if resolved == mode
        ));
        let descriptor = state
            .active_overlay_descriptor()
            .expect("semantic replacement");
        assert_eq!(descriptor.category, ShellOverlayCategory::PagePopover);
        state.refresh_hit_map();
        let enter = state.route_key_input(&KeyInput::from_label("Enter"));
        assert_eq!(enter.0, RoutedTarget::Component(ShellComponent::Explorer));
        assert_eq!(enter.1, ShellCommand::ExplorerOverlayActivate);
        assert_eq!(
            state.route_key_input(&KeyInput::from_label("Esc")).1,
            ShellCommand::ClosePopup
        );
        assert_eq!(state.focus_order(), vec![ShellComponent::Explorer]);
        assert!(
            state
                .hit_map()
                .regions()
                .iter()
                .all(|region| region.component != ShellComponent::ContextMenu)
        );
        assert!(
            state
                .hit_map()
                .regions()
                .iter()
                .any(|region| region.component == ShellComponent::Explorer)
        );
        state.refresh_hit_map_with_motion(ui::MotionTransitions {
            overlay: Some(replacing(ui::MotionTransitionKind::Popover)),
            ..ui::MotionTransitions::default()
        });
        assert_eq!(state.route_key_input(&KeyInput::from_label("Enter")), enter);
        assert_eq!(state.focus_order(), vec![ShellComponent::Explorer]);
        assert_eq!(
            state
                .hit_map()
                .regions()
                .iter()
                .filter(|region| region.layer == ShellHitLayer::AppOverlay)
                .map(|region| region.area)
                .collect::<Vec<_>>(),
            vec![explorer_overlay_surface(&state)]
        );
        let explorer_before = state.app.explorer_state().cloned();
        let overlay_before = state.active_overlay_descriptor();
        let calls_before = platform.calls();
        for point in [(1, 1), (covered.right() - 1, covered.y)] {
            assert!(!rect_contains(explorer_overlay_surface(&state), point));
            let now = Instant::now();
            state.apply_input_with_platform_at(
                InputEvent::mouse_down(PointerButton::Left, point),
                &platform,
                now,
            );
            state.apply_input_with_platform_at(
                InputEvent::mouse_up(PointerButton::Left, point),
                &platform,
                now + Duration::from_millis(20),
            );
            assert_eq!(state.app.explorer_state(), explorer_before.as_ref());
            assert_eq!(state.pages.explorer.explorer_overlay_mode, Some(mode));
            assert_eq!(state.active_overlay_descriptor(), overlay_before);
            assert_eq!(platform.calls(), calls_before);
        }
    }

    for dialog in [
        app::explorer::ExplorerDialog::delete(std::path::Path::new("/delete")),
        app::explorer::ExplorerDialog::dump_trash(2),
    ] {
        let mut state = explorer_routing_test_state();
        state.pages.common.active_popup = Some(ShellPopup {
            owner: Some(ShellComponent::Explorer),
            anchor: (1, 1),
        });
        let mut explorer = state.app.explorer_state().expect("explorer state").clone();
        explorer.pending_dialog = Some(dialog);
        state.app.dispatch_at(
            app::AppCommand::SetExplorerState(Some(explorer)),
            Instant::now(),
        );
        assert!(matches!(
            state.resolved_explorer_overlay(),
            Some(ResolvedExplorerOverlay::PendingDialog(_))
        ));
        let descriptor = state.active_overlay_descriptor().expect("pending dialog");
        assert_eq!(descriptor.kind, ui::MotionOverlayKind::Dialog);
        assert_eq!(descriptor.category, ShellOverlayCategory::PageDialog);
        state.refresh_hit_map();
        let enter = state.route_key_input(&KeyInput::from_label("Enter"));
        assert_eq!(enter.0, RoutedTarget::Component(ShellComponent::Explorer));
        assert!(matches!(
            enter.1,
            ShellCommand::ExplorerConfirmDelete | ShellCommand::ExplorerConfirmDumpTrash
        ));
        assert_eq!(
            state.route_key_input(&KeyInput::from_label("Esc")).1,
            ShellCommand::CancelExplorerInput
        );
        assert_eq!(state.focus_order(), vec![ShellComponent::Explorer]);
        assert!(
            state
                .hit_map()
                .regions()
                .iter()
                .all(|region| region.component != ShellComponent::ContextMenu)
        );
        assert!(
            state
                .hit_map()
                .regions()
                .iter()
                .any(|region| region.component == ShellComponent::Explorer)
        );
        state.refresh_hit_map_with_motion(ui::MotionTransitions {
            overlay: Some(replacing(ui::MotionTransitionKind::Dialog)),
            ..ui::MotionTransitions::default()
        });
        assert_eq!(state.route_key_input(&KeyInput::from_label("Enter")), enter);
        assert_ne!(
            state.route_key_input(&KeyInput::from_label("Esc")).1,
            ShellCommand::Noop
        );
    }

    let mut generic = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    generic.pages.common.active_popup = Some(ShellPopup {
        owner: Some(ShellComponent::Home),
        anchor: (10, 10),
    });
    generic.refresh_hit_map();
    assert_eq!(
        generic.resolved_overlay_owner(),
        Some(ShellComponent::ContextMenu)
    );
    assert_eq!(generic.focus_order(), vec![ShellComponent::ContextMenu]);
    assert!(
        generic
            .hit_map()
            .regions()
            .iter()
            .any(|region| region.component == ShellComponent::ContextMenu)
    );
    assert_eq!(
        generic.route_key_input(&KeyInput::from_label("Esc")).0,
        RoutedTarget::Popup(ShellComponent::ContextMenu)
    );
}

#[test]
fn explorer_popover_wheel_and_right_click_keep_the_covered_list_unchanged() {
    for mode in [
        ExplorerOverlayMode::Sort { anchor: (3, 3) },
        ExplorerOverlayMode::Options,
        ExplorerOverlayMode::Properties,
        ExplorerOverlayMode::ContextMenu { anchor: (3, 3) },
    ] {
        let (mut state, platform, _guard) = explorer_terminal_test_fixture();
        let row = explorer_test_layout(&state).rows[0].area;
        state.open_explorer_popup(mode, (3, 3));
        state.refresh_hit_map_with_motion(ui::MotionTransitions {
            overlay: Some(ui::MotionTransition {
                kind: ui::MotionTransitionKind::Popover,
                direction: ui::MotionDirection::Replacing,
                progress: 0,
                phase_progress: 0,
                active: true,
                next_redraw_in: Duration::from_millis(16),
            }),
            ..Default::default()
        });
        let before = state.app.explorer_state().cloned();
        let overlay = state.active_overlay_descriptor();
        let calls_before = platform.calls();
        let surface = explorer_overlay_surface(&state);
        for point in [
            (row.right() - 1, row.y),
            (
                surface.x + surface.width / 2,
                surface.y + surface.height / 2,
            ),
        ] {
            for input in [
                InputEvent::mouse_scroll(ScrollDirection::Down, point),
                InputEvent::mouse_scroll(ScrollDirection::Up, point),
                InputEvent::mouse_down(PointerButton::Right, point),
                InputEvent::mouse_up(PointerButton::Right, point),
            ] {
                state.apply_input_with_platform(input, &platform);
                assert_eq!(state.app.explorer_state(), before.as_ref());
                assert_eq!(state.active_overlay_descriptor(), overlay);
                assert_eq!(state.pages.explorer.explorer_overlay_mode, Some(mode));
                assert_eq!(platform.calls(), calls_before);
            }
        }
        assert_eq!(state.focus_order(), vec![ShellComponent::Explorer]);
        state.apply_input_with_platform(InputEvent::from_key_label("Esc"), &platform);
        assert!(state.resolved_explorer_overlay().is_none());
        assert_eq!(state.app.explorer_state(), before.as_ref());
    }
}

#[test]
fn explorer_conflict_arrows_select_the_action_instead_of_always_keeping_both() {
    let mut state = explorer_routing_test_state();
    state.update_explorer_state(|explorer| {
        explorer.pending_conflict = Some(app::explorer::ExplorerConflict {
            source: "/source".into(),
            target: "/target".into(),
            remaining: 1,
        });
    });
    state.refresh_hit_map();
    state.apply_input(InputEvent::from_key_label("Right"));
    let Some(ui::ExplorerOverlayViewModel::Conflict(conflict)) =
        state.to_explorer_view_model().overlay
    else {
        panic!("expected conflict dialog");
    };
    assert_eq!(
        conflict.selected_choice,
        ui::ExplorerConflictChoice::Replace
    );
    assert_eq!(
        state.route_key_input(&KeyInput::from_label("Enter")).1,
        ShellCommand::ExplorerOverlayActivate
    );
}

#[test]
fn explorer_dialog_navigation_wraps_and_preserves_input_focus() {
    for mode in [
        ExplorerInputMode::NewFolder,
        ExplorerInputMode::NewTextFile,
        ExplorerInputMode::Rename,
        ExplorerInputMode::RestoreDestination,
    ] {
        let mut state = explorer_routing_test_state();
        state.begin_explorer_input(mode);
        state.pages.explorer.explorer_input = "界ab".into();
        state.pages.explorer.explorer_address_cursor = 1;
        state.pages.explorer.explorer_address_selection_anchor = None;
        state.refresh_hit_map();
        for (key, cursor) in [("Right", 2), ("Left", 1), ("Home", 0), ("End", 3)] {
            state.apply_input(InputEvent::from_key_label(key));
            assert_eq!(
                state.pages.explorer.explorer_overlay_selection, 0,
                "{mode:?}: {key}"
            );
            assert_eq!(
                state.pages.explorer.explorer_address_cursor, cursor,
                "{mode:?}: {key}"
            );
            assert_eq!(state.pages.explorer.explorer_input, "界ab");
        }
        for (key, selected) in [
            ("Tab", 1),
            ("Right", 2),
            ("Left", 1),
            ("Down", 2),
            ("Tab", 0),
            ("Up", 2),
            ("Shift+Tab", 1),
            ("Shift+Tab", 0),
        ] {
            state.apply_input(InputEvent::from_key_label(key));
            assert_eq!(
                state.to_explorer_view_model().overlay_selection,
                selected,
                "{mode:?}: {key}"
            );
        }
        state.apply_input(InputEvent::from_key_label("Tab"));
        assert_eq!(
            state.route_key_input(&KeyInput::from_label("Enter")).1,
            ShellCommand::ExplorerEditInput(InputEvent::from_key_label("Enter"))
        );
        state.apply_input(InputEvent::from_key_label("Shift+Tab"));
        assert_eq!(state.pages.explorer.explorer_overlay_selection, 0);
        state.apply_input(InputEvent::from_key_label("Shift+Tab"));
        assert_eq!(state.pages.explorer.explorer_overlay_selection, 2);
        state.apply_input(InputEvent::from_key_label("x"));
        assert_eq!(state.pages.explorer.explorer_input, "界ab");
        state.apply_input(InputEvent::from_key_label("Enter"));
        assert!(state.to_explorer_view_model().overlay.is_none());
        assert_eq!(state.active_screen(), ShellScreen::Explorer);
        state.begin_explorer_input(mode);
        state.refresh_hit_map();
        assert_eq!(
            state.route_key_input(&KeyInput::from_label("Enter")).1,
            ShellCommand::ExplorerEditInput(InputEvent::from_key_label("Enter"))
        );
        state.apply_input(InputEvent::from_key_label("Esc"));
        assert!(state.to_explorer_view_model().overlay.is_none());
        assert_eq!(
            state.pages.explorer.explorer_input_mode,
            ExplorerInputMode::Browse
        );
    }
}

#[test]
fn explorer_conflict_navigation_reaches_apply_to_remaining_and_supports_reverse_tab() {
    let mut state = explorer_routing_test_state();
    state.update_explorer_state(|explorer| {
        explorer.pending_conflict = Some(app::explorer::ExplorerConflict {
            source: "/source".into(),
            target: "/target".into(),
            remaining: 2,
        });
    });
    state.refresh_hit_map();
    state.apply_input(InputEvent::from_key_label("Shift+Tab"));
    assert_eq!(state.pages.explorer.explorer_overlay_selection, 4);
    state.apply_input(InputEvent::from_key_label(" "));
    assert!(state.pages.explorer.explorer_conflict_apply_to_remaining);
    state.apply_input(InputEvent::from_key_label("Enter"));
    assert!(!state.pages.explorer.explorer_conflict_apply_to_remaining);
    state.apply_input(InputEvent::from_key_label("Right"));
    assert_eq!(state.pages.explorer.explorer_overlay_selection, 0);
    state.apply_input(InputEvent::from_key_label("Up"));
    assert_eq!(state.pages.explorer.explorer_overlay_selection, 4);
}

#[test]
fn explorer_options_close_is_keyboard_reachable() {
    let mut state = explorer_routing_test_state();
    state.open_explorer_popup(ExplorerOverlayMode::Options, (0, 0));
    state.apply_input(InputEvent::from_key_label("Shift+Tab"));
    let Some(ui::ExplorerOverlayViewModel::Options(options)) =
        state.to_explorer_view_model().overlay
    else {
        panic!("options");
    };
    assert_eq!(
        state.pages.explorer.explorer_overlay_selection,
        options.options.len()
    );
    state.apply_input(InputEvent::from_key_label("Enter"));
    assert!(state.to_explorer_view_model().overlay.is_none());
}

#[test]
fn restore_conflict_cycles_only_its_visible_actions() {
    let mut state = explorer_routing_test_state();
    state.update_explorer_state(|explorer| {
        explorer.pending_restore = Some(app::explorer::ExplorerPendingRestore {
            id: platform::mock::MockPlatform::trash_entry_id("restoring"),
            display_name: "alpha.txt".into(),
            target: "/alpha.txt".into(),
        });
    });
    state.refresh_hit_map();
    for (key, choice) in [
        ("Right", ui::ExplorerConflictChoice::Replace),
        ("Down", ui::ExplorerConflictChoice::Cancel),
        ("Tab", ui::ExplorerConflictChoice::KeepBoth),
        ("Shift+Tab", ui::ExplorerConflictChoice::Cancel),
    ] {
        state.apply_input(InputEvent::from_key_label(key));
        let Some(ui::ExplorerOverlayViewModel::Conflict(conflict)) =
            state.to_explorer_view_model().overlay
        else {
            panic!("restore conflict");
        };
        assert_eq!(conflict.selected_choice, choice);
        assert!(!conflict.allow_apply_to_remaining);
    }
    assert_eq!(
        state.route_key_input(&KeyInput::from_label("Enter")).1,
        ShellCommand::ExplorerOverlayActivate
    );
}
