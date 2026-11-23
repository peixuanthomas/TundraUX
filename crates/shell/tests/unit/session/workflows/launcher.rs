use super::*;

#[test]
fn launcher_keeps_cached_icons_until_explicit_theme_refresh() {
    use std::fs;

    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let parent = std::env::temp_dir().canonicalize().unwrap().join(format!(
        "tundra-shell-launcher-assets-{}-{unique}",
        std::process::id()
    ));
    fs::create_dir(&parent).expect("create Launcher asset fixture");
    let fixture = SystemStatusTempGuard(parent);
    let root = fixture.0.join("assets");
    let parked = fixture.0.join("offline-assets");
    ui::restore_default_theme(&root).expect("create complete default theme fixture");
    let app_icons = root.join("themes/default/app_icons.toml");
    let source = fs::read_to_string(&app_icons).expect("read Launcher icons");
    let first = source.replacen("' ______ '", "'CACHE001'", 1);
    assert_ne!(first, source);
    fs::write(&app_icons, &first).expect("write initial cached Launcher icon");

    let assets = ui::RuntimeAsciiAssets::load_with_root(&root, ui::DEFAULT_THEME_ID)
        .expect("load initial asset cache");
    fs::rename(&root, &parked).expect("make the runtime asset directory unavailable");
    let startup = ShellStartupState::clean(
        PlatformKind::Windows,
        PlatformCapabilities::native_supported(),
    );
    let mut state = ShellSession::new_with_runtime_services(
        ShellLaunchConfig::default(),
        (120, 40),
        startup,
        assets,
        ShellRuntimeServices {
            language: None,
            terminal_image_support: true,
            explorer: None,
            diagnostics: None,
            editor: ShellEditorTaskRuntime::unavailable(),
            settings: ShellSettingsTaskRuntime::unavailable(),
            window_owner: Default::default(),
            inherited_auth: None,
            session_snapshot: None,
        },
        None,
    );
    set_test_auth_role(&mut state, UserRole::Admin);

    let assert_icon = |state: &ShellSession, expected: &str| {
        let launcher = state.to_launcher_view_model();
        let item = launcher
            .items
            .iter()
            .find(|item| item.id == app::COMMAND_LINE_APPLICATION.id)
            .expect("Command Line Launcher entry");
        let icon = launcher.item_icon(item).expect("Command Line ASCII icon");
        assert!(icon.lines().iter().any(|line| line == expected), "{icon:?}");
        assert!(
            launcher
                .item_graphic_bytes(item)
                .is_some_and(|bytes| bytes.starts_with(b"\x89PNG\r\n\x1a\n")),
            "Command Line PNG should be available"
        );
    };
    assert!(
        !app_icons.exists(),
        "theme files should still be unavailable"
    );
    assert_icon(&state, "CACHE001");

    // Startup can recreate locales at the original root while the theme is absent.
    fs::create_dir_all(&root).expect("restore the runtime root");
    fs::rename(parked.join("themes"), root.join("themes")).expect("return the theme files");
    let second = first.replacen("CACHE001", "CACHE002", 1);
    fs::write(&app_icons, second).expect("change Launcher icon after startup");
    assert_icon(&state, "CACHE001");

    state
        .refresh_asset_cache_for_theme(ui::DEFAULT_THEME_ID)
        .expect("explicit cache refresh");
    assert_icon(&state, "CACHE002");
}

#[test]
fn launcher_confirmation_buttons_support_keyboard_and_mouse_cancellation() {
    for pending in [
        LauncherPendingConfirmation::Launch {
            id: "app".into(),
            path: "/app".into(),
            kind: LauncherExecutableKind::NativeBinary,
            new_session: false,
        },
        LauncherPendingConfirmation::Remove {
            ids: vec!["app".into()],
            label: "app".into(),
        },
    ] {
        for key in ["Right", "Left", "Up", "Down", "Tab", "Shift+Tab"] {
            let mut state = explorer_routing_test_state();
            state.set_navigation_path(vec![ShellScreen::Launcher]);
            state.pages.common.focused_component = ShellComponent::Launcher;
            state.pages.launcher.launcher_pending_confirmation = Some(pending.clone());
            state.refresh_hit_map();
            state.apply_input(InputEvent::from_key_label(key));
            assert!(
                !state
                    .to_launcher_view_model()
                    .confirmation
                    .unwrap()
                    .confirm_selected,
                "{key}"
            );
            state.apply_input(InputEvent::from_key_label(" "));
            assert!(state.pages.launcher.launcher_pending_confirmation.is_none());
            assert_eq!(
                state.last_command(),
                Some(&ShellCommand::LauncherCancelConfirmation)
            );
            state.pages.launcher.launcher_pending_confirmation = Some(pending.clone());
            state.refresh_hit_map();
            let ui::ShellLayout::Full { main, .. } =
                ui::compute_shell_layout(Rect::new(0, 0, 120, 40))
            else {
                panic!("full layout");
            };
            let cancel = ui::launcher_layout(main, &state.to_launcher_view_model())
                .confirmation
                .unwrap()
                .cancel;
            state.apply_input(InputEvent::mouse_down(
                PointerButton::Left,
                (cancel.x, cancel.y),
            ));
            assert!(state.pages.launcher.launcher_pending_confirmation.is_some());
            state.apply_input(InputEvent::mouse_up(
                PointerButton::Left,
                (cancel.x, cancel.y),
            ));
            assert!(state.pages.launcher.launcher_pending_confirmation.is_none());
        }
    }
}

#[test]
fn linux_personal_apps_use_process_permissions_without_admin_role() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut state, UserRole::User);
    let mut session = state.app.auth_session().unwrap().clone();
    session.source = identity::IdentitySource::LinuxCurrentProcess;
    state.app.dispatch_at(
        app::AppCommand::SetAuthSession(Some(session)),
        Instant::now(),
    );
    state.platform_kind = PlatformKind::Linux;
    assert!(state.to_logs_view_model().can_view_system);
    assert!(state.can_manage_launcher());
    assert!(state.can_execute_command_line());
    assert!(state.can_change_global_settings());
}

#[test]
fn launcher_arrow_keys_follow_grid_rows_and_columns() {
    for size in [(116, 41), (144, 24), (108, 20)] {
        let layout = |state: &ShellSession| {
            let ui::ShellLayout::Full { main, .. } =
                state.shell_layout_for(Rect::new(0, 0, size.0, size.1))
            else {
                panic!("launcher navigation requires a full layout");
            };
            ui::launcher_layout(main, &state.to_launcher_view_model())
        };
        for view in [
            ui::LauncherViewMode::LargeIcons,
            ui::LauncherViewMode::Details,
        ] {
            for sorted in [false, true] {
                let mut state = ShellSession::new(ShellLaunchConfig::default(), size);
                set_test_auth_role(&mut state, UserRole::Admin);
                state.set_navigation_path(vec![ShellScreen::Home, ShellScreen::Launcher]);
                state.pages.common.focused_component = ShellComponent::Launcher;
                state.pages.launcher.launcher_view_mode = view;
                state.app.dispatch_at(
                    app::AppCommand::SetLauncherState(Some(LauncherState {
                        items: (0..18)
                            .map(|index| app::launcher::LauncherItem {
                                record: storage::LauncherEntryRecord {
                                    id: format!("navigation-{index}"),
                                    path: format!("/navigation/app-{index}"),
                                    terminal: None,
                                    custom: None,
                                    name: None,
                                    icon: None,
                                    executable_kind: None,
                                    added_by_user_id: "admin".into(),
                                    added_at_epoch_ms: 0,
                                },
                                status: app::launcher::LauncherItemStatus::Ready,
                            })
                            .collect(),
                        ..LauncherState::default()
                    })),
                    Instant::now(),
                );
                if sorted {
                    state.sort_active_table(0);
                }
                state.apply_input(InputEvent::key(InputKey::Down));
                let columns = layout(&state).columns;
                let last = state.launcher_item_count() - 1;
                for (start, key, expected) in [
                    (0, "Down", columns),
                    (columns, "Up", 0),
                    (0, "Right", 1),
                    (1, "Left", 0),
                    (columns - 1, "Up", columns - 1),
                    (last, "Down", last),
                    (last / columns * columns - 1, "Down", last),
                ] {
                    state.select_launcher_index(start);
                    state.apply_input(InputEvent::from_key_label(key));
                    assert_eq!(
                        state.to_launcher_view_model().selected_index,
                        Some(expected),
                        "size={size:?}, view={view:?}, sorted={sorted}, start={start}, key={key}"
                    );
                    assert!(
                        layout(&state)
                            .items
                            .iter()
                            .any(|item| item.index == expected),
                        "selected item must remain visible"
                    );
                }
            }
        }
    }
}

#[test]
fn launcher_sort_preserves_builtin_identity_and_keyboard_order() {
    let mut state = ShellSession::new(ShellLaunchConfig::default(), (120, 40));
    set_test_auth_role(&mut state, UserRole::Admin);
    state.set_navigation_path(vec![ShellScreen::Home, ShellScreen::Launcher]);
    state.pages.common.focused_component = ShellComponent::Launcher;
    state.pages.launcher.launcher_view_mode = ui::LauncherViewMode::Details;
    let original_id = app::EDITOR_APPLICATION.id;
    state.pages.launcher.launcher_selected_index = state
        .to_launcher_view_model()
        .items
        .iter()
        .position(|item| item.id == original_id)
        .unwrap();
    state.sort_active_table(0);
    let model = state.to_launcher_view_model();
    assert_eq!(model.items[model.selected_index.unwrap()].id, original_id);
    assert!(
        model
            .items
            .windows(2)
            .all(|pair| ui::compare_table_cells(&pair[0].name, &pair[1].name)
                != std::cmp::Ordering::Greater)
    );
    state.select_launcher_last();
    assert_eq!(
        state.selected_built_in_launcher_application().unwrap().id,
        model.items.last().unwrap().id
    );
    state.select_launcher_delta(-1);
    assert_eq!(
        state.selected_built_in_launcher_application().unwrap().id,
        model.items[model.items.len() - 2].id
    );
    let ui::ShellLayout::Full { main, .. } = state.shell_layout_for(Rect::new(0, 0, 120, 40))
    else {
        panic!()
    };
    let layout = ui::launcher_layout(main, &state.to_launcher_view_model());
    let header = ui::launcher_sort_headers(&layout, &state.to_launcher_view_model())[0].1;
    let at = Instant::now();
    state.apply_input_at(
        InputEvent::mouse_down(PointerButton::Left, (header.x, header.y)),
        at,
    );
    assert!(!state.pages.common.table_sorts["launcher"].descending);
    state.apply_input_at(
        InputEvent::mouse_up(PointerButton::Left, (header.x, header.y)),
        at + Duration::from_millis(20),
    );
    assert!(state.pages.common.table_sorts["launcher"].descending);
}
