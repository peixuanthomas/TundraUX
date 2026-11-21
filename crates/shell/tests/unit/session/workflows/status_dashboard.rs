use super::*;

#[test]
fn system_status_arrow_navigation_follows_mixed_size_card_edges() {
    use crate::session::controller::system_status::ui_widget_kind;
    use storage::SystemStatusWidgetKind::*;

    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut state, UserRole::User);
    state.enter_screen(ShellScreen::SystemStatus);
    state.pages.common.focused_component = ShellComponent::SystemStatus;

    for editing in [false, true] {
        if editing {
            state.begin_system_status_dashboard_edit();
            state
                .pages
                .system_status
                .system_status_dashboard_draft
                .as_mut()
                .unwrap()
                .wide
                .placements
                .reverse();
        }
        for (from, targets) in [
            (
                SystemOverview,
                [SystemOverview, Cpu, SystemOverview, Storage],
            ),
            (Cpu, [SystemOverview, Memory, Cpu, Diagnostics]),
            (Memory, [Cpu, Memory, Memory, Diagnostics]),
            (Storage, [Storage, Network, SystemOverview, UptimeLoad]),
            (Network, [Storage, Diagnostics, SystemOverview, UptimeLoad]),
            (Diagnostics, [Network, Diagnostics, Cpu, Diagnostics]),
            (UptimeLoad, [UptimeLoad, Diagnostics, Storage, UptimeLoad]),
        ] {
            for (key, expected) in ["Left", "Right", "Up", "Down"].into_iter().zip(targets) {
                state.pages.system_status.system_status_selected_widget = Some(from);
                state.restore_system_status_widget_focus();
                state.apply_input(InputEvent::from_key_label(key));
                assert_eq!(
                    state.pages.system_status.system_status_selected_widget,
                    Some(expected),
                    "{from:?} {key}, editing={editing}"
                );
                assert_eq!(
                    state.pages.system_status.system_status_dashboard_focus,
                    ui::SystemStatusDashboardFocus::Widget(ui_widget_kind(expected))
                );
            }
        }
    }
}

#[test]
fn system_status_arrow_navigation_handles_staggered_cards_and_gaps() {
    use storage::{SystemStatusWidgetKind::*, SystemStatusWidgetSize::*, WidgetPlacement};

    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut state, UserRole::User);
    state.enter_screen(ShellScreen::SystemStatus);
    state.pages.common.focused_component = ShellComponent::SystemStatus;

    for (placements, key, expected) in [
        (
            [
                (Cpu, 0, 2, Small),
                (Memory, 4, 3, Small),
                (Storage, 2, 0, Small),
            ],
            "Right",
            Memory,
        ),
        (
            [
                (Cpu, 2, 0, Small),
                (Memory, 3, 4, Small),
                (Storage, 0, 2, Small),
            ],
            "Down",
            Memory,
        ),
        (
            [
                (Cpu, 4, 0, Wide),
                (Memory, 2, 2, Large),
                (Storage, 6, 3, Small),
            ],
            "Down",
            Memory,
        ),
        (
            [
                (Cpu, 0, 0, Small),
                (Memory, 2, 10, Small),
                (Storage, 3, 3, Small),
            ],
            "Right",
            Storage,
        ),
    ] {
        let config = storage::SystemStatusDashboardConfig {
            widgets: placements.iter().map(|p| p.0).collect(),
            wide: storage::DashboardLayout {
                placements: placements
                    .into_iter()
                    .map(|(kind, column, row, size)| WidgetPlacement {
                        kind,
                        column,
                        row,
                        size,
                    })
                    .collect(),
            },
            narrow: storage::DashboardLayout::default(),
        };
        state.app.dispatch_at(
            app::AppCommand::SetActiveSystemStatusDashboard(Some(config)),
            Instant::now(),
        );
        state.pages.system_status.system_status_selected_widget = Some(Cpu);
        state.restore_system_status_widget_focus();
        state.apply_input(InputEvent::from_key_label(key));
        assert_eq!(
            state.pages.system_status.system_status_selected_widget,
            Some(expected),
            "{placements:?}, {key}"
        );
    }
}

#[test]
fn system_status_arrow_navigation_reaches_offscreen_cards_and_scroll_clamps() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (100, 20),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut state, UserRole::Admin);
    state.enter_screen(ShellScreen::SystemStatus);
    state.pages.common.focused_component = ShellComponent::SystemStatus;
    state.begin_system_status_dashboard_edit();
    let mut draft = state
        .pages
        .system_status
        .system_status_dashboard_draft
        .take()
        .unwrap();
    draft.add_widget(storage::SystemStatusWidgetKind::Battery);
    let battery = draft
        .wide
        .placements
        .iter_mut()
        .find(|p| p.kind == storage::SystemStatusWidgetKind::Battery)
        .unwrap();
    battery.column = 0;
    battery.row = 20;
    state.pages.system_status.system_status_dashboard_draft = Some(draft);
    state.pages.system_status.system_status_selected_widget =
        Some(storage::SystemStatusWidgetKind::UptimeLoad);
    state.pages.system_status.system_status_dashboard_focus =
        ui::SystemStatusDashboardFocus::Widget(ui::SystemStatusWidgetKind::UptimeLoad);
    state.select_system_status_widget_direction(0, 1);
    assert_eq!(
        state.pages.system_status.system_status_selected_widget,
        Some(storage::SystemStatusWidgetKind::Battery)
    );
    assert!(state.pages.system_status.system_status_dashboard_scroll_row > 0);
    state.select_system_status_widget_direction(0, -1);
    assert_ne!(
        state.pages.system_status.system_status_selected_widget,
        Some(storage::SystemStatusWidgetKind::Battery)
    );
    let focused = state
        .pages
        .system_status
        .system_status_selected_widget
        .unwrap();
    let (_, layout) = state.system_status_layout().unwrap();
    let widget = state
        .to_system_status_view_model()
        .unwrap()
        .dashboard
        .wide_widgets
        .into_iter()
        .find(|w| w.kind == crate::session::controller::system_status::ui_widget_kind(focused))
        .unwrap();
    assert!(
        widget.row >= layout.visible_row_start
            && widget.row + widget.size.rows() <= layout.visible_row_end
    );

    state.pages.system_status.system_status_selected_widget =
        Some(storage::SystemStatusWidgetKind::Battery);
    state.scroll_system_status_focused_widget_into_view(ui::SystemStatusWidgetKind::Battery);
    state.remove_selected_system_status_widget();
    let (_, layout) = state.system_status_layout().unwrap();
    assert_eq!(
        state.pages.system_status.system_status_dashboard_scroll_row,
        layout.visible_row_start
    );
    assert!(!layout.widgets.is_empty());
    let replacement = crate::session::controller::system_status::ui_widget_kind(
        state
            .pages
            .system_status
            .system_status_selected_widget
            .unwrap(),
    );
    assert!(
        layout
            .widgets
            .iter()
            .any(|widget| widget.kind == replacement)
    );

    state.pages.system_status.system_status_dashboard_scroll_row = 20;
    state.terminal_size = (80, 20);
    state.clamp_system_status_dashboard_scroll();
    let (_, narrow) = state.system_status_layout().unwrap();
    assert_eq!(
        state.pages.system_status.system_status_dashboard_scroll_row,
        narrow.visible_row_start
    );
    assert!(!narrow.widgets.is_empty());
}

#[test]
fn system_status_add_scrolls_new_bottom_widget_fully_into_view() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 20),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut state, UserRole::Admin);
    state.enter_screen(ShellScreen::SystemStatus);
    state.pages.common.focused_component = ShellComponent::SystemStatus;
    state.begin_system_status_dashboard_edit();
    let index = crate::session::controller::system_status::SYSTEM_STATUS_WIDGET_KINDS
        .iter()
        .position(|kind| *kind == storage::SystemStatusWidgetKind::Temperature)
        .unwrap();
    state.open_system_status_add_picker();
    state.select_system_status_picker_item(index);
    state.add_selected_system_status_widget();
    assert_eq!(
        state.pages.system_status.system_status_selected_widget,
        Some(storage::SystemStatusWidgetKind::Temperature)
    );
    assert_eq!(
        state.pages.system_status.system_status_dashboard_focus,
        ui::SystemStatusDashboardFocus::Widget(ui::SystemStatusWidgetKind::Temperature)
    );
    assert!(state.pages.system_status.system_status_dashboard_scroll_row > 0);
    let (model, layout) = state.system_status_layout().unwrap();
    let activity = model
        .dashboard
        .widgets(layout.profile)
        .iter()
        .find(|widget| widget.kind == ui::SystemStatusWidgetKind::Temperature)
        .unwrap();
    assert!(activity.row >= layout.visible_row_start);
    assert!(activity.row + activity.size.rows() <= layout.visible_row_end);
}

#[test]
fn system_status_short_add_picker_click_uses_visible_absolute_index() {
    for kind in [
        ui::MouseEventKind::Down(PointerButton::Left),
        ui::MouseEventKind::Click(PointerButton::Left),
        ui::MouseEventKind::DoubleClick(PointerButton::Left),
    ] {
        let mut state = ShellSession::new_for_home_mode(
            ShellLaunchConfig::default(),
            (120, 12),
            ShellHomeMode::User,
        );
        set_test_auth_role(&mut state, UserRole::Admin);
        state.enter_screen(ShellScreen::SystemStatus);
        state.pages.common.focused_component = ShellComponent::SystemStatus;
        state.begin_system_status_dashboard_edit();
        state.open_system_status_add_picker();
        state.select_system_status_picker_item(5);
        let (model, layout) = state.system_status_layout().unwrap();
        assert_eq!(
            model.dashboard.picker.as_ref().unwrap().items[5].kind,
            ui::SystemStatusWidgetKind::Temperature
        );
        let row = layout
            .picker_items
            .iter()
            .find(|row| row.index == 5)
            .unwrap()
            .area;
        state.apply_input(InputEvent::Mouse(ui::MouseEvent::new(row.x, row.y, kind)));
        assert_eq!(
            state.pages.system_status.system_status_selected_widget,
            Some(storage::SystemStatusWidgetKind::Temperature)
        );
        let draft = state
            .pages
            .system_status
            .system_status_dashboard_draft
            .as_ref()
            .unwrap();
        assert_eq!(
            draft
                .widgets
                .iter()
                .filter(|kind| **kind == storage::SystemStatusWidgetKind::Temperature)
                .count(),
            1
        );
        assert!(state.pages.system_status.system_status_add_picker.is_none());
    }
}

#[test]
fn system_status_disabled_add_and_picker_rows_are_inert() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut state, UserRole::Admin);
    state.enter_screen(ShellScreen::SystemStatus);
    state.pages.common.focused_component = ShellComponent::SystemStatus;
    state.begin_system_status_dashboard_edit();
    for kind in crate::session::controller::system_status::SYSTEM_STATUS_WIDGET_KINDS {
        state
            .pages
            .system_status
            .system_status_dashboard_draft
            .as_mut()
            .unwrap()
            .add_widget(kind);
    }
    let model = state.to_system_status_view_model().unwrap();
    assert!(model.dashboard.actions.add_disabled);
    assert_eq!(
        state.route_key_input(&KeyInput::from_label("a")).1,
        ShellCommand::Noop
    );
    let ui::ShellLayout::Full { main, .. } = ui::compute_shell_layout(Rect::new(0, 0, 120, 40))
    else {
        panic!()
    };
    let layout = ui::system_status_layout(main, &model);
    state.apply_input(InputEvent::Mouse(ui::MouseEvent::new(
        layout.add_button.x,
        layout.add_button.y,
        ui::MouseEventKind::Down(PointerButton::Left),
    )));
    assert!(state.pages.system_status.system_status_add_picker.is_none());

    state.finish_cancel_system_status_dashboard_edit();
    state.begin_system_status_dashboard_edit();
    state.open_system_status_add_picker();
    let selected = state
        .pages
        .system_status
        .system_status_add_picker
        .as_ref()
        .unwrap()
        .selected;
    let before = state
        .pages
        .system_status
        .system_status_dashboard_draft
        .clone()
        .unwrap();
    let model = state.to_system_status_view_model().unwrap();
    let layout = ui::system_status_layout(main, &model);
    let disabled = layout
        .picker_items
        .iter()
        .find(|row| !model.dashboard.picker.as_ref().unwrap().items[row.index].enabled)
        .unwrap();
    for kind in [
        ui::MouseEventKind::Down(PointerButton::Left),
        ui::MouseEventKind::Click(PointerButton::Left),
        ui::MouseEventKind::Up(PointerButton::Left),
        ui::MouseEventKind::Down(PointerButton::Right),
        ui::MouseEventKind::Down(PointerButton::Middle),
        ui::MouseEventKind::DoubleClick(PointerButton::Left),
    ] {
        state.apply_input(InputEvent::Mouse(ui::MouseEvent::new(
            disabled.area.x,
            disabled.area.y,
            kind,
        )));
    }
    assert_eq!(
        state
            .pages
            .system_status
            .system_status_add_picker
            .as_ref()
            .unwrap()
            .selected,
        selected
    );
    assert_eq!(
        state
            .pages
            .system_status
            .system_status_dashboard_draft
            .as_ref()
            .unwrap(),
        &before
    );
    state.activate_system_status_picker_item(disabled.index);
    assert_eq!(
        state
            .pages
            .system_status
            .system_status_add_picker
            .as_ref()
            .unwrap()
            .selected,
        selected
    );
    assert_eq!(
        state
            .pages
            .system_status
            .system_status_dashboard_draft
            .as_ref()
            .unwrap(),
        &before
    );
}

#[test]
fn system_status_clean_save_is_inert_for_shortcut_mouse_and_direct_call() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut state, UserRole::Admin);
    state.enter_screen(ShellScreen::SystemStatus);
    state.pages.common.focused_component = ShellComponent::SystemStatus;
    state.begin_system_status_dashboard_edit();
    let draft = state
        .pages
        .system_status
        .system_status_dashboard_draft
        .clone();
    let active = state.app.active_system_status_dashboard().cloned();
    let focus = state.pages.system_status.system_status_dashboard_focus;
    let feedback = state
        .pages
        .system_status
        .system_status_dashboard_feedback
        .clone();
    assert_eq!(
        state
            .route_key_input(&KeyInput::with_modifiers(
                InputKey::Char('s'),
                InputModifiers::CTRL
            ))
            .1,
        ShellCommand::Noop
    );
    let model = state.to_system_status_view_model().unwrap();
    assert!(model.dashboard.actions.save_disabled);
    let ui::ShellLayout::Full { main, .. } = ui::compute_shell_layout(Rect::new(0, 0, 120, 40))
    else {
        panic!()
    };
    let save = ui::system_status_layout(main, &model).save_button;
    for kind in [
        ui::MouseEventKind::Down(PointerButton::Left),
        ui::MouseEventKind::Click(PointerButton::Left),
    ] {
        state.apply_input(InputEvent::Mouse(ui::MouseEvent::new(save.x, save.y, kind)));
    }
    state.save_system_status_dashboard();
    assert_eq!(
        state.pages.system_status.system_status_dashboard_draft,
        draft
    );
    assert_eq!(state.app.active_system_status_dashboard().cloned(), active);
    assert_eq!(
        state.pages.system_status.system_status_dashboard_focus,
        focus
    );
    assert_eq!(
        state.pages.system_status.system_status_dashboard_feedback,
        feedback
    );
}

#[test]
fn system_status_add_picker_shows_full_unavailable_reasons_to_users() {
    let sentinel = "/sensitive/backend/path secret-device";
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut state, UserRole::User);
    let mut snapshot = system_status_metric_snapshot(10);
    snapshot.metrics.thermal = system_services::MetricState::Unavailable {
        reason: sentinel.into(),
    };
    snapshot.metrics.batteries = system_services::MetricState::Unavailable {
        reason: sentinel.into(),
    };
    state.apply_system_status_snapshot(snapshot);
    state.begin_system_status_dashboard_edit();
    let config = state
        .pages
        .system_status
        .system_status_dashboard_draft
        .clone()
        .unwrap();
    state.open_system_status_add_picker();
    let user = state
        .to_system_status_view_model()
        .unwrap()
        .dashboard
        .picker
        .unwrap();
    let temperature = user
        .items
        .iter()
        .find(|item| item.kind == ui::SystemStatusWidgetKind::Temperature)
        .unwrap();
    let battery = user
        .items
        .iter()
        .find(|item| item.kind == ui::SystemStatusWidgetKind::Battery)
        .unwrap();
    assert_eq!(temperature.detail, sentinel);
    assert_eq!(battery.detail, sentinel);
    assert!(!temperature.enabled && !battery.enabled);
    assert!(format!("{:?}", user).contains(sentinel));
    assert_eq!(
        state
            .pages
            .system_status
            .system_status_dashboard_draft
            .as_ref()
            .unwrap(),
        &config
    );

    set_test_auth_role(&mut state, UserRole::Admin);
    let admin = state
        .to_system_status_view_model()
        .unwrap()
        .dashboard
        .picker
        .unwrap();
    for kind in [
        ui::SystemStatusWidgetKind::Temperature,
        ui::SystemStatusWidgetKind::Battery,
    ] {
        let item = admin.items.iter().find(|item| item.kind == kind).unwrap();
        assert_eq!(item.detail, sentinel);
        assert!(!item.enabled);
    }
    assert_eq!(
        state
            .pages
            .system_status
            .system_status_dashboard_draft
            .as_ref()
            .unwrap(),
        &config
    );
}

#[test]
fn system_status_right_click_opens_contextual_edit_pickers() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut state, UserRole::Admin);
    state.enter_screen(ShellScreen::SystemStatus);
    state.pages.common.focused_component = ShellComponent::SystemStatus;

    let (_, layout) = state.system_status_layout().unwrap();
    let widget = layout
        .widgets
        .iter()
        .find(|widget| !widget.preview)
        .copied()
        .unwrap();
    let widget_point = (widget.area.x + 1, widget.area.y + 1);
    state.apply_input(InputEvent::mouse_down(PointerButton::Right, widget_point));

    assert!(
        state
            .pages
            .system_status
            .system_status_dashboard_draft
            .is_some()
    );
    assert_eq!(
        state.pages.system_status.system_status_selected_widget,
        Some(crate::session::controller::system_status::storage_widget_kind(widget.kind))
    );
    assert_eq!(
        state
            .pages
            .system_status
            .system_status_size_picker
            .as_ref()
            .unwrap()
            .anchor,
        Some(widget_point)
    );
    let contextual_layout = ui::system_status_layout(
        match ui::compute_shell_layout(Rect::new(0, 0, 120, 40)) {
            ui::ShellLayout::Full { main, .. } => main,
            _ => panic!(),
        },
        &state.to_system_status_view_model().unwrap(),
    );
    assert_eq!(
        contextual_layout.size_picker_items[0].area.x,
        widget_point.0 + ui::components::DialogLayout::INSET
    );

    let size_row = contextual_layout.size_picker_items[2].area;
    let size_point = (size_row.x, size_row.y);
    state.apply_input(InputEvent::mouse_down(PointerButton::Left, size_point));
    state.apply_input(InputEvent::mouse_up(PointerButton::Left, size_point));
    assert!(
        state
            .pages
            .system_status
            .system_status_size_picker
            .is_none()
    );
    let draft = state
        .pages
        .system_status
        .system_status_dashboard_draft
        .as_ref()
        .unwrap();
    assert_eq!(
        draft
            .wide
            .placements
            .iter()
            .find(|p| p.kind
                == crate::session::controller::system_status::storage_widget_kind(widget.kind))
            .unwrap()
            .size,
        storage::SystemStatusWidgetSize::Large
    );
    assert!(
        state
            .pages
            .system_status
            .system_status_widget_drag
            .is_none()
    );
    state.finish_cancel_system_status_dashboard_edit();
    let layout = ui::system_status_layout(
        match ui::compute_shell_layout(Rect::new(0, 0, 120, 40)) {
            ui::ShellLayout::Full { main, .. } => main,
            _ => panic!(),
        },
        &state.to_system_status_view_model().unwrap(),
    );
    let blank_point = (layout.canvas.y..layout.canvas.bottom())
        .flat_map(|y| (layout.canvas.x..layout.canvas.right()).map(move |x| (x, y)))
        .find(|point| ui::system_status_hit_test(&layout, *point).is_none())
        .expect("dashboard has blank canvas space");
    state.apply_input(InputEvent::mouse_down(PointerButton::Right, blank_point));

    assert!(
        state
            .pages
            .system_status
            .system_status_dashboard_draft
            .is_none()
    );
    let baseline = state.system_status_dashboard_config();
    let focus = state.pages.system_status.system_status_dashboard_focus;
    state.apply_input(InputEvent::key(InputKey::Escape));
    assert!(state.pages.system_status.system_status_add_picker.is_none());
    assert!(
        state
            .pages
            .system_status
            .system_status_dashboard_draft
            .is_none()
    );
    assert_eq!(state.system_status_dashboard_config(), baseline);
    assert_eq!(
        state.pages.system_status.system_status_dashboard_focus,
        focus
    );
    state.apply_input(InputEvent::mouse_down(PointerButton::Right, blank_point));
    assert!(
        state
            .pages
            .system_status
            .system_status_dashboard_draft
            .is_none()
    );
    // Activating an already-added item must not enter edit mode.
    state.activate_system_status_picker_item(0);
    assert!(
        state
            .pages
            .system_status
            .system_status_dashboard_draft
            .is_none()
    );
    assert_eq!(
        state
            .pages
            .system_status
            .system_status_add_picker
            .as_ref()
            .unwrap()
            .anchor,
        Some(blank_point)
    );
    assert!(
        state
            .pages
            .system_status
            .system_status_size_picker
            .is_none()
    );
    let (model, layout) = state.system_status_layout().unwrap();
    let picker = model.dashboard.picker.as_ref().unwrap();
    let row = layout
        .picker_items
        .iter()
        .find(|row| picker.items[row.index].kind == ui::SystemStatusWidgetKind::Temperature)
        .unwrap();
    let point = (row.area.x, row.area.y);
    state.apply_input(InputEvent::mouse_down(PointerButton::Left, point));
    assert!(state.pages.system_status.system_status_add_picker.is_none());
    let draft = state
        .pages
        .system_status
        .system_status_dashboard_draft
        .clone()
        .unwrap();
    state.apply_input(InputEvent::mouse_up(PointerButton::Left, point));
    assert_eq!(
        state
            .pages
            .system_status
            .system_status_dashboard_draft
            .as_ref(),
        Some(&draft)
    );
    assert_eq!(
        draft
            .widgets
            .iter()
            .filter(|kind| **kind == storage::SystemStatusWidgetKind::Temperature)
            .count(),
        1
    );
    assert!(
        state
            .pages
            .system_status
            .system_status_widget_drag
            .is_none()
    );
}

#[test]
fn system_status_dashboard_focus_wraps_skips_disabled_and_activates() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut state, UserRole::Admin);
    state.enter_screen(ShellScreen::SystemStatus);
    state.pages.common.focused_component = ShellComponent::SystemStatus;
    state.ensure_system_status_widget_selection();
    state.restore_system_status_widget_focus();
    let first = state.pages.system_status.system_status_dashboard_focus;
    state.pages.system_status.system_status_dashboard_focus =
        ui::SystemStatusDashboardFocus::Diagnostics;
    state.move_system_status_dashboard_focus(1);
    assert_eq!(
        state.pages.system_status.system_status_dashboard_focus, first,
        "forward wraps"
    );
    state.move_system_status_dashboard_focus(-1);
    assert_eq!(
        state.pages.system_status.system_status_dashboard_focus,
        ui::SystemStatusDashboardFocus::Diagnostics,
        "reverse wraps"
    );

    state.begin_system_status_dashboard_edit();
    state.pages.system_status.system_status_selected_widget = None;
    let first_edit_focus = state.system_status_dashboard_focus_order()[0];
    state.pages.system_status.system_status_dashboard_focus = ui::SystemStatusDashboardFocus::Add;
    state.move_system_status_dashboard_focus(1);
    assert_eq!(
        state.pages.system_status.system_status_dashboard_focus,
        ui::SystemStatusDashboardFocus::Cancel
    );
    state.move_system_status_dashboard_focus(1);
    assert_eq!(
        state.pages.system_status.system_status_dashboard_focus, first_edit_focus,
        "disabled Size, Remove, and clean Save are skipped before wrapping"
    );
    assert!(
        state
            .pages
            .system_status
            .system_status_selected_widget
            .is_some()
    );
    state.move_system_status_dashboard_focus(-1);
    assert_eq!(
        state.pages.system_status.system_status_dashboard_focus,
        ui::SystemStatusDashboardFocus::Cancel
    );
    state.move_system_status_dashboard_focus(-1);
    assert_eq!(
        state.pages.system_status.system_status_dashboard_focus,
        ui::SystemStatusDashboardFocus::Remove,
        "focusing a widget enables Size and Remove; clean Save remains skipped"
    );
    state.pages.system_status.system_status_selected_widget = None;
    state.pages.system_status.system_status_dashboard_focus = first_edit_focus;
    state.move_system_status_dashboard_focus(-1);
    assert_eq!(
        state.pages.system_status.system_status_dashboard_focus,
        ui::SystemStatusDashboardFocus::Cancel
    );
    state.move_system_status_dashboard_focus(-1);
    assert_eq!(
        state.pages.system_status.system_status_dashboard_focus,
        ui::SystemStatusDashboardFocus::Add,
        "without a selected widget reverse wrapping skips Size, Remove, and clean Save"
    );
    state.apply_input(InputEvent::from_key_label("Enter"));
    assert!(
        state.pages.system_status.system_status_add_picker.is_some(),
        "focused Add activates"
    );
    state.apply_input(InputEvent::from_key_label("Esc"));
    assert!(state.pages.system_status.system_status_add_picker.is_none());
    assert!(
        state
            .pages
            .system_status
            .system_status_dashboard_draft
            .is_some()
    );
    state.apply_input(InputEvent::from_key_label("Esc"));
    assert!(
        state
            .pages
            .system_status
            .system_status_dashboard_draft
            .is_none(),
        "Escape cancels clean dashboard edits"
    );
}

#[test]
fn system_status_dashboard_focus_restores_and_tabs_offscreen() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (100, 20),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut state, UserRole::Admin);
    state.enter_screen(ShellScreen::SystemStatus);
    state.pages.common.focused_component = ShellComponent::SystemStatus;
    state.ensure_system_status_widget_selection();
    let kind = state
        .pages
        .system_status
        .system_status_selected_widget
        .expect("default widget");
    state.pages.system_status.system_status_dashboard_focus =
        ui::SystemStatusDashboardFocus::Widget(
            crate::session::controller::system_status::ui_widget_kind(kind),
        );
    state.open_system_status_detail(kind);
    state.back_from_system_status_detail();
    assert_eq!(
        state.pages.system_status.system_status_dashboard_focus,
        ui::SystemStatusDashboardFocus::Widget(
            crate::session::controller::system_status::ui_widget_kind(kind)
        )
    );

    state.begin_system_status_dashboard_edit();
    let draft = state
        .pages
        .system_status
        .system_status_dashboard_draft
        .as_mut()
        .unwrap();
    let last = draft.wide.placements.last_mut().unwrap();
    last.row = 20;
    let last_kind = crate::session::controller::system_status::ui_widget_kind(last.kind);
    let order = state.system_status_dashboard_focus_order();
    let position = order
        .iter()
        .position(|focus| *focus == ui::SystemStatusDashboardFocus::Widget(last_kind))
        .unwrap();
    state.pages.system_status.system_status_dashboard_focus = order[position.saturating_sub(1)];
    state.move_system_status_dashboard_focus(1);
    assert_eq!(
        state.pages.system_status.system_status_dashboard_focus,
        ui::SystemStatusDashboardFocus::Widget(last_kind)
    );
    assert!(
        state.pages.system_status.system_status_dashboard_scroll_row > 0,
        "Tab scrolls the offscreen widget into view"
    );
}

#[test]
fn system_status_size_shortcut_cycles_and_picker_applies_active_profile_only() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut state, UserRole::Admin);
    state.enter_screen(ShellScreen::SystemStatus);
    state.pages.common.focused_component = ShellComponent::SystemStatus;
    state.begin_system_status_dashboard_edit();
    state.pages.system_status.system_status_selected_widget =
        Some(storage::SystemStatusWidgetKind::Cpu);
    state.pages.system_status.system_status_dashboard_focus =
        ui::SystemStatusDashboardFocus::Widget(ui::SystemStatusWidgetKind::Cpu);
    let size = |state: &ShellSession, profile| {
        state
            .pages
            .system_status
            .system_status_dashboard_draft
            .as_ref()
            .unwrap()
            .layout(profile)
            .placements
            .iter()
            .find(|p| p.kind == storage::SystemStatusWidgetKind::Cpu)
            .unwrap()
            .size
    };
    let before = size(&state, storage::DashboardProfile::Wide);
    state.apply_input(InputEvent::from_key_label("s"));
    assert_eq!(
        size(&state, storage::DashboardProfile::Wide),
        before.cycle(),
        "S remains immediate cycle"
    );

    state.pages.system_status.system_status_dashboard_focus = ui::SystemStatusDashboardFocus::Size;
    state.apply_input(InputEvent::from_key_label("Enter"));
    let expected = match size(&state, storage::DashboardProfile::Wide) {
        storage::SystemStatusWidgetSize::Small => 0,
        storage::SystemStatusWidgetSize::Wide => 1,
        storage::SystemStatusWidgetSize::Large => 2,
    };
    assert_eq!(
        state
            .pages
            .system_status
            .system_status_size_picker
            .as_ref()
            .unwrap()
            .selected,
        expected
    );
    let unchanged = state
        .pages
        .system_status
        .system_status_dashboard_draft
        .clone()
        .unwrap();
    state.apply_input(InputEvent::from_key_label("Esc"));
    assert_eq!(
        state
            .pages
            .system_status
            .system_status_dashboard_draft
            .as_ref()
            .unwrap(),
        &unchanged,
        "Esc does not resize"
    );

    let narrow_before = size(&state, storage::DashboardProfile::Narrow);
    state.open_system_status_size_picker();
    state.select_system_status_size_picker_item(2);
    state.apply_system_status_size_picker();
    assert_eq!(
        size(&state, storage::DashboardProfile::Wide),
        storage::SystemStatusWidgetSize::Large
    );
    assert_eq!(
        size(&state, storage::DashboardProfile::Narrow),
        narrow_before,
        "other profile is unchanged"
    );
    assert_eq!(
        state.pages.system_status.system_status_dashboard_focus,
        ui::SystemStatusDashboardFocus::Size
    );
}

#[test]
fn system_status_size_picker_click_applies_exact_size_and_transitions_clear_it() {
    for kind in [
        ui::MouseEventKind::Down(PointerButton::Left),
        ui::MouseEventKind::Click(PointerButton::Left),
        ui::MouseEventKind::DoubleClick(PointerButton::Left),
    ] {
        let mut state = ShellSession::new_for_home_mode(
            ShellLaunchConfig::default(),
            (120, 40),
            ShellHomeMode::User,
        );
        set_test_auth_role(&mut state, UserRole::Admin);
        state.enter_screen(ShellScreen::SystemStatus);
        state.pages.common.focused_component = ShellComponent::SystemStatus;
        state.begin_system_status_dashboard_edit();
        state.pages.system_status.system_status_selected_widget =
            Some(storage::SystemStatusWidgetKind::Cpu);
        state.open_system_status_size_picker();
        let model = state.to_system_status_view_model().unwrap();
        let ui::ShellLayout::Full { main, .. } = ui::compute_shell_layout(Rect::new(0, 0, 120, 40))
        else {
            panic!()
        };
        let row = ui::system_status_layout(main, &model).size_picker_items[2].area;
        state.apply_input(InputEvent::Mouse(ui::MouseEvent::new(row.x, row.y, kind)));
        let wide = state
            .pages
            .system_status
            .system_status_dashboard_draft
            .as_ref()
            .unwrap()
            .wide
            .placements
            .iter()
            .find(|p| p.kind == storage::SystemStatusWidgetKind::Cpu)
            .unwrap();
        assert_eq!(wide.size, storage::SystemStatusWidgetSize::Large);
        assert!(
            state
                .pages
                .system_status
                .system_status_size_picker
                .is_none()
        );

        state.open_system_status_size_picker();
        state.open_system_status_add_picker();
        assert!(
            state
                .pages
                .system_status
                .system_status_size_picker
                .is_none()
        );
        state.close_system_status_add_picker();
        state.open_system_status_size_picker();
        state.finish_cancel_system_status_dashboard_edit();
        assert!(
            state
                .pages
                .system_status
                .system_status_size_picker
                .is_none()
        );
    }
}

#[test]
fn system_status_draft_profiles_catalog_cancel_and_save_failure_are_isolated() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (80, 24),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut state, UserRole::Admin);
    state.enter_screen(ShellScreen::SystemStatus);
    state.pages.common.focused_component = ShellComponent::SystemStatus;
    let baseline = state.system_status_dashboard_config();
    state.begin_system_status_dashboard_edit();
    state.pages.system_status.system_status_selected_widget =
        Some(storage::SystemStatusWidgetKind::Cpu);
    state.move_selected_system_status_widget(1, 2);
    state.cycle_selected_system_status_widget_size();
    let changed = state
        .pages
        .system_status
        .system_status_dashboard_draft
        .as_ref()
        .unwrap();
    assert_eq!(
        changed.wide, baseline.wide,
        "narrow edits leave wide semantics identical"
    );
    assert_ne!(changed.narrow, baseline.narrow);
    state.finish_cancel_system_status_dashboard_edit();
    assert_eq!(
        state.system_status_dashboard_config(),
        baseline,
        "Cancel restores baseline"
    );

    state.begin_system_status_dashboard_edit();
    let add_kind = storage::SystemStatusWidgetKind::Battery;
    let picker_index = crate::session::controller::system_status::SYSTEM_STATUS_WIDGET_KINDS
        .iter()
        .position(|kind| *kind == add_kind)
        .unwrap();
    state.open_system_status_add_picker();
    state.select_system_status_picker_item(picker_index);
    state.apply_input(InputEvent::from_key_label("Enter"));
    let added = state
        .pages
        .system_status
        .system_status_dashboard_draft
        .as_ref()
        .unwrap()
        .clone();
    assert_eq!(
        added
            .widgets
            .iter()
            .filter(|kind| **kind == add_kind)
            .count(),
        1
    );
    for layout in [&added.wide, &added.narrow] {
        assert_eq!(
            layout
                .placements
                .iter()
                .filter(|p| p.kind == add_kind)
                .count(),
            1
        );
    }
    state.remove_selected_system_status_widget();
    let removed = state
        .pages
        .system_status
        .system_status_dashboard_draft
        .as_ref()
        .unwrap();
    assert!(!removed.widgets.contains(&add_kind));
    assert_eq!(
        removed.wide, baseline.wide,
        "removal does not compact unrelated wide gaps"
    );
    assert_eq!(
        removed.narrow, baseline.narrow,
        "removal does not compact unrelated narrow gaps"
    );

    state.pages.system_status.system_status_selected_widget =
        Some(storage::SystemStatusWidgetKind::Cpu);
    state.cycle_selected_system_status_widget_size();
    let dirty = state
        .pages
        .system_status
        .system_status_dashboard_draft
        .clone()
        .unwrap();
    state.save_system_status_dashboard();
    assert_eq!(
        state
            .pages
            .system_status
            .system_status_dashboard_draft
            .as_ref(),
        Some(&dirty)
    );
    assert_eq!(state.app.active_system_status_dashboard(), None);
    assert!(
        state
            .pages
            .system_status
            .system_status_dashboard_feedback
            .as_ref()
            .map(i18n::LocalizedText::render_current)
            .unwrap_or_default()
            .contains("Storage unavailable")
    );
}

#[test]
fn system_status_drag_uses_rendered_column_stride_at_the_gap_boundary() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut state, UserRole::User);
    state.enter_screen(ShellScreen::SystemStatus);
    state.begin_system_status_dashboard_edit();
    let draft = state
        .pages
        .system_status
        .system_status_dashboard_draft
        .as_mut()
        .unwrap();
    draft.widgets = vec![storage::SystemStatusWidgetKind::Cpu];
    draft.wide.placements = vec![storage::WidgetPlacement {
        kind: storage::SystemStatusWidgetKind::Cpu,
        column: 0,
        row: 0,
        size: storage::SystemStatusWidgetSize::Small,
    }];
    let (_, layout) = state.system_status_layout().unwrap();
    let kind = ui::SystemStatusWidgetKind::Cpu;
    let card = layout
        .widgets
        .iter()
        .find(|card| card.kind == kind)
        .unwrap();
    state.begin_system_status_widget_drag(
        kind,
        (card.area.x + layout.column_stride - 1, card.area.y + 1),
    );
    assert_eq!(
        state
            .pages
            .system_status
            .system_status_widget_drag
            .as_ref()
            .unwrap()
            .grab_column,
        0
    );
    state.update_system_status_widget_drag((
        layout.canvas.x + layout.column_stride * 2 - 1,
        layout.canvas.y + (ui::LOGICAL_ROW_HEIGHT + ui::LOGICAL_ROW_GAP) * 3 + 1,
    ));
    let draft = state
        .pages
        .system_status
        .system_status_dashboard_draft
        .as_ref()
        .unwrap();
    let placement = draft
        .wide
        .placements
        .iter()
        .find(|placement| placement.kind == storage::SystemStatusWidgetKind::Cpu)
        .unwrap();
    assert_eq!((placement.column, placement.row), (1, 3));
}

#[test]
fn system_status_drag_changes_only_active_profile_and_clears_capture() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut state, UserRole::User);
    state.enter_screen(ShellScreen::SystemStatus);
    state.pages.common.focused_component = ShellComponent::SystemStatus;
    state.begin_system_status_dashboard_edit();
    state.pages.system_status.system_status_selected_widget =
        Some(storage::SystemStatusWidgetKind::Cpu);
    let inactive = state
        .pages
        .system_status
        .system_status_dashboard_draft
        .as_ref()
        .unwrap()
        .narrow
        .clone();
    let before = state
        .pages
        .system_status
        .system_status_dashboard_draft
        .as_ref()
        .unwrap()
        .wide
        .clone();
    let model = state.to_system_status_view_model().unwrap();
    let ui::ShellLayout::Full { main, .. } = ui::compute_shell_layout(Rect::new(0, 0, 120, 40))
    else {
        panic!()
    };
    let layout = ui::system_status_layout(main, &model);
    let card = layout
        .widgets
        .iter()
        .find(|widget| widget.kind == ui::SystemStatusWidgetKind::Cpu)
        .unwrap();
    let down = (card.area.x.saturating_add(1), card.area.y.saturating_add(1));
    let target = (
        down.0,
        down.1
            .saturating_add(6)
            .min(layout.canvas.bottom().saturating_sub(1)),
    );
    let started_at = Instant::now();
    state.apply_input_at(
        InputEvent::mouse_down(PointerButton::Left, down),
        started_at,
    );
    assert!(
        state
            .pages
            .system_status
            .system_status_widget_drag
            .is_some()
    );
    state.apply_input_at(
        InputEvent::mouse_drag(PointerButton::Left, target),
        started_at + Duration::from_millis(20),
    );
    state.apply_input_at(
        InputEvent::mouse_up(PointerButton::Left, target),
        started_at + Duration::from_millis(40),
    );
    assert!(
        state
            .pages
            .system_status
            .system_status_widget_drag
            .is_none()
    );
    let draft = state
        .pages
        .system_status
        .system_status_dashboard_draft
        .as_ref()
        .unwrap();
    assert_ne!(draft.wide, before);
    assert_eq!(draft.narrow, inactive);
    let rendered = state.to_system_status_view_model().unwrap();
    let layout = ui::system_status_layout(main, &rendered);
    assert!(!rendered.dashboard.actions.save_disabled);
    let save_coordinates = (layout.save_button.x, layout.save_button.y);
    assert_eq!(
        ui::system_status_hit_test(&layout, save_coordinates),
        Some(ui::SystemStatusHitTarget::Save)
    );
    assert_eq!(
        state.hit_map.layer_at(save_coordinates),
        Some(ShellHitLayer::AppContent)
    );
    assert_eq!(
        state
            .route_input_at(
                InputEvent::mouse_down(PointerButton::Left, save_coordinates),
                started_at + Duration::from_millis(60),
            )
            .command,
        ShellCommand::SystemStatusSaveDashboard
    );
    state.apply_input_at(
        InputEvent::from_key_label("Esc"),
        started_at + Duration::from_millis(80),
    );
    assert!(state.pages.system_status.system_status_discard_dialog);
    for (index, left) in layout.widgets.iter().filter(|w| !w.preview).enumerate() {
        for right in layout.widgets.iter().filter(|w| !w.preview).skip(index + 1) {
            assert!(
                left.area.intersection(right.area).is_empty(),
                "resolved placements do not overlap"
            );
        }
    }
}

#[test]
fn system_status_clipped_widget_click_release_preserves_drag_origin() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 20),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut state, UserRole::Admin);
    state.enter_screen(ShellScreen::SystemStatus);
    state.pages.common.focused_component = ShellComponent::SystemStatus;
    state.begin_system_status_dashboard_edit();
    state.pages.system_status.system_status_dashboard_scroll_row = 1;
    state.clamp_system_status_dashboard_scroll();
    let (model, layout) = state.system_status_layout().unwrap();
    assert_eq!(layout.visible_row_start, 1);
    let overview = model
        .dashboard
        .widgets(layout.profile)
        .iter()
        .find(|widget| {
            widget.kind == ui::SystemStatusWidgetKind::SystemOverview
                && widget.row == 0
                && widget.size.rows() > 1
        })
        .unwrap();
    let clipped = layout
        .widgets
        .iter()
        .find(|widget| widget.kind == overview.kind && !widget.preview)
        .unwrap();
    assert_eq!(clipped.area.y, layout.canvas.y);
    let profile = crate::session::controller::system_status::storage_profile(layout.profile);
    let before = state
        .pages
        .system_status
        .system_status_dashboard_draft
        .as_ref()
        .unwrap()
        .layout(profile)
        .placements
        .clone();
    let pointer = (clipped.area.x.saturating_add(1), clipped.area.y);
    state.begin_system_status_widget_drag(overview.kind, pointer);
    assert!(
        state
            .pages
            .system_status
            .system_status_widget_drag
            .is_some()
    );
    state.finish_system_status_widget_drag(pointer);
    assert!(
        state
            .pages
            .system_status
            .system_status_widget_drag
            .is_none()
    );
    let after = state
        .pages
        .system_status
        .system_status_dashboard_draft
        .as_ref()
        .unwrap()
        .layout(profile)
        .placements
        .clone();
    assert_eq!(
        after, before,
        "pointer down/up without movement preserves every placement"
    );
}

#[test]
fn system_status_save_persists_dashboard_to_user_record() {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "tundra-shell-dashboard-save-{}-{unique}",
        std::process::id()
    ));
    let _guard = SystemStatusTempGuard(root.clone());
    let paths = platform::build_linux_app_paths(
        root.join("Config"),
        root.join("Data"),
        root.join("Cache"),
        root.join("State"),
        root.join("Temp"),
    )
    .unwrap();
    let manager = StorageManager::open(paths).unwrap().manager;
    let session = seed_system_profile(
        &manager,
        "DashboardAdmin",
        storage::AppearanceConfig::default(),
    );
    let mut startup = ShellStartupState::clean(
        PlatformKind::Linux,
        PlatformCapabilities::native_supported(),
    );
    startup.storage_manager = Some(manager.clone());
    let mut state =
        ShellSession::new_with_startup(ShellLaunchConfig::default(), (120, 40), startup);
    state.attach_system_session(session);
    let loaded = state
        .app
        .active_system_status_dashboard()
        .cloned()
        .expect("login loads dashboard");
    state.enter_screen(ShellScreen::SystemStatus);
    state.pages.common.focused_component = ShellComponent::SystemStatus;
    state.begin_system_status_dashboard_edit();
    let index = crate::session::controller::system_status::SYSTEM_STATUS_WIDGET_KINDS
        .iter()
        .position(|kind| *kind == storage::SystemStatusWidgetKind::Battery)
        .unwrap();
    state.open_system_status_add_picker();
    state.select_system_status_picker_item(index);
    state.apply_input(InputEvent::from_key_label("Enter"));
    let expected = state
        .pages
        .system_status
        .system_status_dashboard_draft
        .clone()
        .unwrap();
    assert_ne!(expected, loaded);
    state.save_system_status_dashboard();
    assert!(
        state
            .pages
            .system_status
            .system_status_dashboard_draft
            .is_none()
    );
    assert_eq!(state.app.active_system_status_dashboard(), Some(&expected));
    let persisted = manager
        .load_profile("DashboardAdmin")
        .unwrap()
        .unwrap()
        .system_status_dashboard;
    assert_eq!(persisted, expected);
}

#[test]
fn system_status_mouse_wheel_and_scrollbar_drag_update_explicit_viewport() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (80, 16),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut state, UserRole::Admin);
    let mut snapshot = system_status_test_snapshot(
        1,
        system_services::StoragePressure::Normal,
        true,
        system_services::SystemVolumeSource::Detected,
    );
    let system_services::StorageState::Ready(ref mut storage) = snapshot.storage else {
        unreachable!()
    };
    let template = storage.volumes[0].clone();
    storage.volumes = (0..30)
        .map(|index| system_services::StorageVolumeSnapshot {
            identifier: format!("volume-{index}"),
            is_system: index == 0,
            ..template.clone()
        })
        .collect();
    state.apply_system_status_snapshot(snapshot);
    state.enter_screen(ShellScreen::SystemStatus);
    state.pages.common.focused_component = ShellComponent::SystemStatus;
    state.set_system_status_tab(ui::SystemStatusTab::Storage);
    state.refresh_hit_map();
    state.apply_input(InputEvent::from_key_label("End"));
    let model = state.to_system_status_view_model().unwrap();
    let ui::ShellLayout::Full { main, .. } = ui::compute_shell_layout(Rect::new(0, 0, 80, 16))
    else {
        panic!()
    };
    let keyboard_layout = ui::system_status_layout(main, &model);
    assert!(keyboard_layout.visible_start > 1);
    assert_eq!(state.pages.system_status.system_status_scroll_offset, 0);
    state.apply_input(InputEvent::Mouse(ui::MouseEvent::new(
        10,
        8,
        ui::MouseEventKind::Scroll(ScrollDirection::Up),
    )));
    let after_wheel_model = state.to_system_status_view_model().unwrap();
    let after_wheel_layout = ui::system_status_layout(main, &after_wheel_model);
    assert_eq!(
        after_wheel_layout.visible_start,
        keyboard_layout.visible_start.saturating_sub(1)
    );
    assert!(
        state.pages.system_status.system_status_selected_row >= after_wheel_layout.visible_start
    );
    assert!(
        state.pages.system_status.system_status_selected_row
            < after_wheel_layout.visible_start + after_wheel_layout.visible_capacity
    );
    state.apply_input(InputEvent::Mouse(ui::MouseEvent::new(
        10,
        8,
        ui::MouseEventKind::Scroll(ScrollDirection::Down),
    )));
    assert!(state.pages.system_status.system_status_scroll_offset > 0);
    let model = state.to_system_status_view_model().unwrap();
    let layout = ui::system_status_layout(main, &model);
    let track = layout.scrollbar.expect("scrollbar");
    state.apply_input(InputEvent::Mouse(ui::MouseEvent::new(
        track.x,
        track.bottom().saturating_sub(1),
        ui::MouseEventKind::Down(PointerButton::Left),
    )));
    assert!(matches!(
        state.scrollbar_drag,
        Some(ScrollbarDragState::SystemStatus { .. })
    ));
    state.apply_input(InputEvent::Mouse(ui::MouseEvent::new(
        track.x,
        track.bottom().saturating_sub(1),
        ui::MouseEventKind::Drag(PointerButton::Left),
    )));
    assert!(state.pages.system_status.system_status_scroll_offset > 1);
    state.apply_input(InputEvent::Mouse(ui::MouseEvent::new(
        track.x,
        track.bottom().saturating_sub(1),
        ui::MouseEventKind::Up(PointerButton::Left),
    )));
    assert!(state.scrollbar_drag.is_none());
}

#[test]
fn system_status_modal_focus_traps_and_restores_page_focus() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut state, UserRole::Admin);
    state.enter_screen(ShellScreen::SystemStatus);
    state.pages.common.focused_component = ShellComponent::SystemStatus;
    state.notify_modal(
        "Confirm",
        "Modal over status",
        ui::NotificationTone::Info,
        vec![ShellNotificationAction::new("ok", "OK")],
    );
    state.refresh_hit_map();
    assert_eq!(
        state.focus_order(),
        vec![ShellComponent::NotificationDialog]
    );
    assert_eq!(
        state.pages.common.focused_component,
        ShellComponent::NotificationDialog
    );
    state.move_focus(ui::FocusDirection::Next);
    assert_eq!(
        state.pages.common.focused_component,
        ShellComponent::NotificationDialog
    );
    state.apply_input(InputEvent::from_key_label("Enter"));
    state.refresh_hit_map();
    assert_eq!(state.focus_order(), vec![ShellComponent::SystemStatus]);
    assert_eq!(
        state.pages.common.focused_component,
        ShellComponent::SystemStatus
    );

    state.show_time_sync_failure_dialog("offline");
    state.refresh_hit_map();
    assert_eq!(state.focus_order(), vec![ShellComponent::TimeSyncDialog]);
    state.move_focus(ui::FocusDirection::Previous);
    assert_eq!(
        state.pages.common.focused_component,
        ShellComponent::TimeSyncDialog
    );
    state.apply_input(InputEvent::from_key_label("Enter"));
    state.refresh_hit_map();
    assert_eq!(
        state.pages.common.focused_component,
        ShellComponent::SystemStatus
    );
}

#[test]
fn system_status_picker_wheel_moves_selection_without_editing_or_scrolling_dashboard() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 12),
        ShellHomeMode::User,
    );
    set_test_auth_role(&mut state, UserRole::Admin);
    state.enter_screen(ShellScreen::SystemStatus);
    state.pages.common.focused_component = ShellComponent::SystemStatus;
    state.begin_system_status_dashboard_edit();
    state.open_system_status_add_picker();
    let draft = state
        .pages
        .system_status
        .system_status_dashboard_draft
        .clone();
    let scroll = state.pages.system_status.system_status_dashboard_scroll_row;
    let selected = state
        .pages
        .system_status
        .system_status_add_picker
        .as_ref()
        .unwrap()
        .selected;
    let (_, layout) = state.system_status_layout().unwrap();
    let area = layout.picker_items[0].area;
    state.apply_input(InputEvent::mouse_scroll(
        ScrollDirection::Down,
        (area.x, area.y),
    ));
    let next = state
        .pages
        .system_status
        .system_status_add_picker
        .as_ref()
        .unwrap()
        .selected;
    assert_ne!(next, selected);
    let (model, layout) = state.system_status_layout().unwrap();
    assert!(model.dashboard.picker.as_ref().unwrap().items[next].enabled);
    let area = layout
        .picker_items
        .iter()
        .find(|row| row.index == next)
        .unwrap()
        .area;
    for kind in [
        ui::MouseEventKind::Moved,
        ui::MouseEventKind::Down(PointerButton::Right),
        ui::MouseEventKind::Down(PointerButton::Middle),
        ui::MouseEventKind::Scroll(ScrollDirection::Left),
        ui::MouseEventKind::Scroll(ScrollDirection::Right),
    ] {
        state.apply_input(InputEvent::Mouse(ui::MouseEvent::new(area.x, area.y, kind)));
        assert_eq!(
            state
                .pages
                .system_status
                .system_status_add_picker
                .as_ref()
                .unwrap()
                .selected,
            next
        );
    }
    state.apply_input(InputEvent::mouse_scroll(
        ScrollDirection::Up,
        (area.x, area.y),
    ));
    assert_eq!(
        state
            .pages
            .system_status
            .system_status_add_picker
            .as_ref()
            .unwrap()
            .selected,
        selected
    );
    assert_eq!(
        state.pages.system_status.system_status_dashboard_draft,
        draft
    );
    assert_eq!(
        state.pages.system_status.system_status_dashboard_scroll_row,
        scroll
    );

    state.close_system_status_add_picker();
    state.open_system_status_size_picker();
    let selected = state
        .pages
        .system_status
        .system_status_size_picker
        .as_ref()
        .unwrap()
        .selected;
    let (_, layout) = state.system_status_layout().unwrap();
    let area = layout.size_picker_items[0].area;
    state.apply_input(InputEvent::mouse_scroll(
        ScrollDirection::Down,
        (area.x, area.y),
    ));
    assert_eq!(
        state
            .pages
            .system_status
            .system_status_size_picker
            .as_ref()
            .unwrap()
            .selected,
        (selected + 1) % 3
    );
    state.apply_input(InputEvent::mouse_scroll(
        ScrollDirection::Up,
        (area.x, area.y),
    ));
    assert_eq!(
        state
            .pages
            .system_status
            .system_status_size_picker
            .as_ref()
            .unwrap()
            .selected,
        selected
    );
    assert_eq!(
        state.pages.system_status.system_status_dashboard_draft,
        draft
    );
    assert_eq!(
        state.pages.system_status.system_status_dashboard_scroll_row,
        scroll
    );
}
