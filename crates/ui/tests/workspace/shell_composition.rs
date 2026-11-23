use ratatui::{Terminal, backend::TestBackend, layout::Rect, widgets::Paragraph};
use ui::*;

fn chrome(width: u16, height: u16) -> ShellChromeViewModel {
    ShellChromeViewModel {
        build_mode: "debug".into(),
        display_mode: HomeDisplayMode::Auth,
        terminal_size: (width, height),
        back_button_hovered: false,
        help_available: false,
        title_focused: false,
        help_button_hovered: false,
        help_button_focused: false,
        status: StatusViewModel {
            status: "系统状态消息很长但不应覆盖时钟".repeat(8),
            toast: None,
            error: None,
            alert_tone: NotificationTone::Info,
            time_button_label: Some("09:30".into()),
            time_button_selected: false,
        },
    }
}
fn row(terminal: &Terminal<TestBackend>, y: u16) -> String {
    (0..terminal.backend().buffer().area.width)
        .map(|x| terminal.backend().buffer()[(x, y)].symbol())
        .collect()
}

#[test]
fn header_keeps_help_and_back_without_a_tasks_entry() {
    let model = chrome(120, 40);
    let context = RenderContext::default();
    let layout = ShellFrameLayout::new(Rect::new(0, 0, 120, 40), None, &context)
        .with_back_button(true, None)
        .with_help_button(true);
    assert!(layout.help_button.unwrap().right() < layout.back_button.unwrap().x);
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| render_shell_chrome(frame, &layout, &model, &context))
        .unwrap();
    assert!(!row(&terminal, 0).contains("Tasks"));
    let collapsed = layout.with_input_keyboard(Rect::new(0, 20, 120, 20), 1000);
    assert_eq!(collapsed.help_button.unwrap().height, 1);
    assert_eq!(collapsed.back_button.unwrap().height, 1);
}

fn render(
    model: &ShellChromeViewModel,
    context: &RenderContext,
) -> (Terminal<TestBackend>, ShellFrameLayout) {
    let (width, height) = model.terminal_size;
    let layout = ShellFrameLayout::new(
        Rect::new(0, 0, width, height),
        model.status.time_button_label.as_deref(),
        context,
    )
    .with_back_button(true, None);
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| {
            frame.render_widget(Paragraph::new("PAGE CONTENT"), layout.main);
            render_shell_chrome(frame, &layout, model, context);
        })
        .unwrap();
    (terminal, layout)
}
#[test]
fn compact_threshold_keeps_escape_above_the_content() {
    for (width, height, compact) in [
        (49, 12, true),
        (50, 11, true),
        (50, 12, false),
        (80, 24, false),
    ] {
        let (terminal, layout) = render(&chrome(width, height), &RenderContext::default());
        assert_eq!(layout.is_compact(), compact);
        assert_eq!(layout.time_button.is_none(), compact);
        assert!(layout.back_button.is_some());
        assert!(layout.status_message.is_some());
        if compact {
            assert_eq!(layout.main.y, 1);
            assert_eq!(layout.main.height, height - 1);
            assert!(row(&terminal, 0).contains("TundraUX"));
            assert!(row(&terminal, 0).contains("◀ Esc"));
            assert!(row(&terminal, layout.main.y).contains("PAGE CONTENT"));
            assert_eq!(
                layout.back_button.unwrap().intersection(layout.main).area(),
                0
            );
            assert_eq!(layout.modal_area(), layout.main);
        }
    }
}

#[test]
fn compact_notice_shows_the_shared_terminal_minimum_in_both_languages() {
    let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ascii-assets/assets");
    for language in ["en-US", "zh-CN"] {
        let language = i18n::LanguageSnapshot::load(&assets, language, 1)
            .unwrap()
            .snapshot;
        let _language = i18n::enter_snapshot(std::sync::Arc::new(language));
        let mut terminal = Terminal::new(TestBackend::new(80, 10)).unwrap();
        terminal
            .draw(|frame| {
                render_compact_home(
                    frame,
                    frame.area(),
                    &chrome(80, 10),
                    &TundraTheme::default(),
                );
            })
            .unwrap();
        let text = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(text.contains(&format!(
            "{}x{}",
            MIN_SHELL_TERMINAL_WIDTH, MIN_SHELL_TERMINAL_HEIGHT
        )));
        assert!(!text.contains("50x12"));
    }
}
#[test]
fn page_transition_keeps_main_chrome_pixels_and_hit_regions_fixed() {
    let idle = RenderContext::default();
    let mut moving = idle.clone();
    moving.transitions.screen = Some(MotionTransition {
        kind: MotionTransitionKind::Page,
        direction: MotionDirection::Entering,
        progress: 100,
        phase_progress: 100,
        active: true,
        next_redraw_in: std::time::Duration::from_millis(16),
    });
    let model = chrome(80, 24);
    let (before, base) = render(&model, &idle);
    let (during, shifted) = render(&model, &moving);
    assert_eq!(shifted.main, base.main);
    assert_eq!(shifted.time_button, base.time_button);
    assert_eq!(shifted.back_button, base.back_button);
    assert_eq!(shifted.status_message, base.status_message);
    for y in [0, 23] {
        assert_eq!(row(&before, y), row(&during, y));
    }
    assert!(row(&during, shifted.main.y).contains("PAGE CONTENT"));
}

#[test]
fn editor_screen_content_uses_the_shared_main_without_erasing_shell_chrome() {
    let model = chrome(80, 24);
    let context = RenderContext::default();
    let editor = EditorViewModel::source("sample.txt", "editor content marker");
    let content = ScreenContent::Editor(&editor);
    let layout = ShellFrameLayout::new(Rect::new(0, 0, 80, 24), Some("09:30"), &context);
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal
        .draw(|frame| {
            content.render_content(frame, &layout, &context, None, None);
            content.render_overlay(frame, &layout, &context);
            render_shell_chrome(frame, &layout, &model, &context);
        })
        .unwrap();
    assert!(row(&terminal, 0).contains("TundraUX"));
    assert!(row(&terminal, 0).contains("09:30"));
    assert!(
        (layout.main.y..layout.main.bottom())
            .any(|y| row(&terminal, y).contains("editor content marker"))
    );
}

fn assert_page_leaves_shell_chrome_untouched(content: ScreenContent<'_>) {
    let name = match content {
        ScreenContent::Home(_) => "Home",
        ScreenContent::Setup(_) => "Setup",
        ScreenContent::Explorer(_) => "Explorer",
        ScreenContent::Launcher(_) => "Launcher",
        ScreenContent::CommandLine(_) => "CommandLine",
        ScreenContent::Editor(_) => "Editor",
        ScreenContent::Script(_) => "Script",
        ScreenContent::Settings(_) => "Settings",
        ScreenContent::Logs(_) => "Logs",
        ScreenContent::Management(_) => "Management",
        ScreenContent::Diagnostics(_) => "Diagnostics",
        ScreenContent::SystemStatus(_) => "SystemStatus",
        ScreenContent::Clock(_) => "Clock",
    };
    for (width, height) in [(120, 40), (80, 24), (50, 12), (49, 11)] {
        for shape in [BorderShape::Rounded, BorderShape::Square] {
            let theme = TundraTheme::default().with_border_shape(shape);
            let context = RenderContext::from_theme(&theme, Default::default(), Default::default());
            let context = content.render_context(&context);
            let model = chrome(width, height);
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| {
                    let layout = ShellFrameLayout::new(
                        frame.area(),
                        model.status.time_button_label.as_deref(),
                        &context,
                    );
                    render_shell_chrome(frame, &layout, &model, &context);
                    let before = frame.buffer_mut().clone();
                    for stage in ["content", "overlay"] {
                        if stage == "content" {
                            content.render_content(frame, &layout, &context, None, None);
                        } else {
                            content.render_overlay(frame, &layout, &context);
                        }
                        for position in frame.area().positions() {
                            if !layout.main.contains(position) {
                                assert_eq!(
                                    frame.buffer_mut()[position], before[position],
                                    "{name} {stage} changed {position:?} outside main at {width}x{height}"
                                );
                            }
                        }
                    }
                })
                .unwrap();
        }
    }
}

#[test]
fn all_pages_and_page_overlays_leave_global_chrome_to_the_compositor() {
    let home = HomeViewModel::user(
        "User",
        "09:30",
        vec![ShellEntry::new("Editor", "Edit text")],
    );
    let mut setup = SetupViewModel {
        scroll_offset: 0,
        step: SetupStep::Language,
        languages: setup_language_options(),
        timezones: setup_timezone_options(),
        selected_language_index: 0,
        selected_timezone_index: 0,
        timezone_window_start: 0,
        focused_field: SetupField::LanguageList,
        defaults_available: false,
        border_shape: BorderShape::Rounded,
        theme_color: ratatui::style::Color::White,
        theme_color_value: "white".into(),
        accent_color: ratatui::style::Color::Cyan,
        accent_color_value: "cyan".into(),
        custom_color_target: None,
        custom_color_editing: Default::default(),
        custom_color_input: String::new(),
        custom_color_valid: true,
        custom_color_conflicts_with_theme: false,
        custom_color_error: None,
        error: None,
    };
    let mut explorer = ExplorerViewModel::new("/tmp", Vec::new(), None);
    let mut launcher = LauncherViewModel::new(Vec::new(), None, LauncherViewMode::Details, true);
    let command_line = CommandLineViewModel::new(CommandLineTerminalSnapshot::blank(120, 40));
    let mut editor = EditorViewModel::source("sample.txt", "editor content marker");
    let mut settings = SettingsViewModel {
        focus: Default::default(),
        form_selected: 0,
        form_interaction: Default::default(),
        selected_application: Default::default(),
        selected_category: SettingsCategory::Appearance,
        selected_field: SettingsField::BorderColor,
        cards: vec![SettingsCardViewModel::new(
            "Appearance",
            vec![SettingsItemViewModel::new(
                SettingsField::BorderColor,
                "Border color",
                "White",
                "Choose the border color",
                SettingsControlKind::Palette,
            )],
        )],
        status: "Ready".into(),
        locked_message: None,
        scroll_offset: 0,
        animation_editor: None,
        number_editor: None,
        picker: None,
        color_editor: None,
        weather_location_editor: None,
        file_extensions_editor: None,
        time_sync_server_editor: None,
        update: None,
    };
    let logs = LogsViewModel::default();
    let mut management = ManagementViewModel::default();
    let mut diagnostics = DiagnosticsViewModel::default();
    let mut system_status = SystemStatusViewModel {
        show_title: false,
        content: SystemStatusContentViewModel::User(UserSystemStatusViewModel {
            storage_status: "Healthy".into(),
            storage_tone: components::ComponentTone::Success,
            system_volume_usage: "42%".into(),
            system_volume_used_percentage: Some(42),
            network_status: "Connected".into(),
            network_tone: components::ComponentTone::Success,
            last_refreshed: "now".into(),
        }),
        diagnostics: DiagnosticsViewModel::default(),
        route: SystemStatusRoute::Dashboard,
        dashboard: SystemStatusDashboardViewModel::default(),
        table_sort: None,
        process_sort: SystemStatusProcessSort::default(),
        selected_row: 0,
        scroll_offset: 0,
        refreshing: false,
        feedback: None,
    };
    let mut clock = ClockViewModel::new("09:30");
    for content in [
        ScreenContent::Home(&home),
        ScreenContent::Setup(&setup),
        ScreenContent::Explorer(&explorer),
        ScreenContent::Launcher(&launcher),
        ScreenContent::CommandLine(&command_line),
        ScreenContent::Editor(&editor),
        ScreenContent::Settings(&settings),
        ScreenContent::Logs(&logs),
        ScreenContent::Management(&management),
        ScreenContent::Diagnostics(&diagnostics),
        ScreenContent::SystemStatus(&system_status),
        ScreenContent::Clock(&clock),
    ] {
        assert_page_leaves_shell_chrome_untouched(content);
    }
    for step in [SetupStep::Timezone, SetupStep::Appearance] {
        setup.step = step;
        assert_page_leaves_shell_chrome_untouched(ScreenContent::Setup(&setup));
    }
    setup.custom_color_target = Some(SetupCustomColorTarget::Theme);
    assert_page_leaves_shell_chrome_untouched(ScreenContent::Setup(&setup));
    explorer.overlay = Some(ExplorerOverlayViewModel::Properties(
        ExplorerPropertiesViewModel {
            title: "Properties".into(),
            properties: Vec::new(),
            close_label: "Close".into(),
        },
    ));
    assert_page_leaves_shell_chrome_untouched(ScreenContent::Explorer(&explorer));
    launcher.confirmation = Some(LauncherConfirmationViewModel {
        kind: LauncherConfirmationKind::Remove,
        title: "Remove entry".into(),
        message: "Remove selected entry?".into(),
        confirm_label: "Remove".into(),
        cancel_label: "Cancel".into(),
        confirm_selected: false,
    });
    assert_page_leaves_shell_chrome_untouched(ScreenContent::Launcher(&launcher));
    for menu in [EditorMenu::File, EditorMenu::Edit, EditorMenu::View] {
        editor.open_menu = Some(menu);
        assert_page_leaves_shell_chrome_untouched(ScreenContent::Editor(&editor));
    }
    settings.color_editor = Some(SettingsColorEditorViewModel {
        editing: Default::default(),
        title: "Color".into(),
        value: "#123456".into(),
        error: None,
    });
    assert_page_leaves_shell_chrome_untouched(ScreenContent::Settings(&settings));
    management.form = Some(ManagementForm {
        title: "Action".into(),
        message: "Confirm action".into(),
        ..Default::default()
    });
    assert_page_leaves_shell_chrome_untouched(ScreenContent::Management(&management));
    for tab in DiagnosticsTab::ALL {
        diagnostics.tab = tab;
        assert_page_leaves_shell_chrome_untouched(ScreenContent::Diagnostics(&diagnostics));
    }
    diagnostics.repair_dialog = Some(DiagnosticsRepairDialogViewModel::default());
    assert_page_leaves_shell_chrome_untouched(ScreenContent::Diagnostics(&diagnostics));
    for kind in SystemStatusWidgetKind::ALL {
        system_status.route = SystemStatusRoute::Detail(kind.detail());
        assert_page_leaves_shell_chrome_untouched(ScreenContent::SystemStatus(&system_status));
    }
    system_status.route = SystemStatusRoute::Dashboard;
    system_status.dashboard.dialog = Some(SystemStatusDialogViewModel {
        title: "Discard?".into(),
        message: "Discard changes?".into(),
        confirm_label: "Discard".into(),
        cancel_label: "Cancel".into(),
        selected_action: 1,
    });
    assert_page_leaves_shell_chrome_untouched(ScreenContent::SystemStatus(&system_status));
    clock.create_dialog = Some(ClockCreateDialogViewModel::default());
    assert_page_leaves_shell_chrome_untouched(ScreenContent::Clock(&clock));
}

#[test]
fn header_navigation_stays_separate_from_keyboard_content() {
    let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ascii-assets/assets");
    let pack = i18n::LanguageSnapshot::load(&assets, "zh-CN", 1).unwrap();
    let _language = i18n::enter_snapshot(std::sync::Arc::new(pack.snapshot));
    for size in [(108, 20), (140, 40)] {
        let model = chrome(size.0, size.1);
        let context = RenderContext::default();
        let layout = ShellFrameLayout::new(Rect::new(0, 0, size.0, size.1), None, &context)
            .with_help_button(true)
            .with_back_button(true, Some(workspace_protocol::BackAction::PreviousLocation));
        assert!(layout.help_button.unwrap().right() <= layout.back_button.unwrap().x);
        let mut terminal = Terminal::new(TestBackend::new(size.0, size.1)).unwrap();
        terminal
            .draw(|frame| render_shell_chrome(frame, &layout, &model, &context))
            .unwrap();
        assert!(
            row(&terminal, 0)
                .replace(' ', "")
                .contains(&i18n::tr!("ui-shell-back-location").replace(' ', ""))
        );
        let collapsed =
            layout.with_input_keyboard(Rect::new(0, size.1 / 2, size.0, size.1 / 2), 1000);
        assert_eq!(collapsed.back_button.unwrap().height, 1);
        assert!(collapsed.back_button.unwrap().bottom() <= collapsed.main.y);
    }
}

#[test]
fn single_line_bars_leave_the_full_main_and_keep_control_regions_separate() {
    for (width, height) in [(108, 20), (120, 36), (180, 50)] {
        for (action, label) in [
            (
                workspace_protocol::BackAction::Cancel,
                i18n::tr!("ui-shell-back-cancel"),
            ),
            (
                workspace_protocol::BackAction::PreviousLocation,
                i18n::tr!("ui-shell-back-location"),
            ),
        ] {
            let model = chrome(width, height);
            let theme = TundraTheme::default();
            let buttons = components::ButtonFrame::new(None, None, &theme);
            let mut context =
                RenderContext::from_theme(&theme, Default::default(), Default::default());
            context.buttons = Some(buttons.clone());
            let layout = ShellFrameLayout::new(
                Rect::new(0, 0, width, height),
                model.status.time_button_label.as_deref(),
                &context,
            )
            .with_help_button(true)
            .with_back_button(true, Some(action));
            assert_eq!(layout.main, Rect::new(0, 1, width, height - 2));
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| render_shell_chrome(frame, &layout, &model, &context))
                .unwrap();
            let regions = buttons.regions();
            let areas = [
                ("shell.power", layout.title),
                ("shell.help", layout.help_button.unwrap()),
                ("shell.back", layout.back_button.unwrap()),
                ("shell.status", layout.status_message.unwrap()),
                ("shell.status.time", layout.time_button.unwrap()),
            ];
            for (index, (id, area)) in areas.iter().enumerate() {
                assert_eq!(area.height, 1);
                assert_eq!(area.intersection(layout.bounds), *area);
                assert!(area.intersection(layout.main).is_empty());
                assert!(
                    regions
                        .iter()
                        .any(|region| region.id.as_str() == *id && region.area == *area)
                );
                for (_, other) in areas.iter().skip(index + 1) {
                    assert!(area.intersection(*other).is_empty());
                }
            }
            assert!(
                row(&terminal, 0)
                    .replace(' ', "")
                    .contains(&label.replace(' ', ""))
            );
            assert!(row(&terminal, 0).contains("09:30"));
        }
    }
}

#[test]
fn keyboard_entry_uses_the_header_and_the_panel_uses_the_bottom_row() {
    for (width, height) in [(108, 20), (120, 36), (180, 50)] {
        let bounds = Rect::new(0, 0, width, height);
        let context = RenderContext::default();
        let layout = ShellFrameLayout::new(bounds, Some("09:30"), &context)
            .with_help_button(true)
            .with_keyboard_launcher(true);
        let launcher = layout.keyboard_launcher.unwrap();
        assert_eq!(launcher.y, 0);
        assert_eq!(launcher.right() + 1, layout.help_button.unwrap().x);
        assert_eq!(layout.status_message.unwrap().right(), launcher.x);
        assert_eq!(layout.main, Rect::new(0, 1, width, height - 2));
        assert_eq!(layout.with_keyboard_launcher(true), layout);
        let hidden = screen_keyboard_input_layout(bounds, false, false);
        assert_eq!(layout.with_input_keyboard(hidden.keyboard, 0), layout);
        for visibility in [1, 100, 500, 999, 1_000] {
            let keyboard = screen_keyboard_input_layout_with_visibility(
                bounds,
                true,
                visibility,
                false,
                Default::default(),
            );
            let shown = layout.with_input_keyboard(keyboard.keyboard, visibility);
            assert_eq!(shown.main.y, 1);
            assert_eq!(shown.main.bottom(), keyboard.keyboard.y);
            assert_eq!(
                shown.status_message.unwrap().right(),
                layout.help_button.unwrap().x
            );
            assert_eq!(shown.time_button, layout.time_button);
            assert!(shown.keyboard_launcher.is_none());
            for button in [shown.back_button, shown.help_button].into_iter().flatten() {
                assert_eq!(button.height, 1);
                assert!(button.bottom() <= shown.main.y);
            }
            assert_eq!(shown.modal_area(), shown.main);
        }
    }
}

#[test]
fn running_windows_leave_the_top_status_visible_and_use_the_full_bottom_row() {
    for (width, height) in [(108, 20), (120, 36), (180, 50)] {
        for error in [false, true] {
            let mut model = chrome(width, height);
            model.status.status = "就绪 e\u{301}".into();
            model.status.error = error.then(|| "出错 e\u{301}".into());
            let context = RenderContext::default();
            let original = ShellFrameLayout::new(
                Rect::new(0, 0, width, height),
                model.status.time_button_label.as_deref(),
                &context,
            )
            .with_help_button(true)
            .with_back_button(true, None)
            .with_keyboard_launcher(true);
            let layout = original.with_workspace_taskbar(true);
            assert_eq!(layout.main, original.main);
            assert_eq!(
                layout.workspace_taskbar,
                Some(Rect::new(0, height - 1, width, 1))
            );
            assert_eq!(layout.status_message, original.status_message);
            let message = layout.status_message.unwrap();
            let keyboard = layout.keyboard_launcher.unwrap();
            let help = layout.help_button.unwrap();
            let time = layout.time_button.unwrap();
            let back = layout.back_button.unwrap();
            let controls = [layout.title, message, keyboard, help, time, back];
            assert!(controls.iter().all(|area| area.y == 0 && area.height == 1));
            assert!(controls.windows(2).all(|pair| pair[0].right() <= pair[1].x));
            assert_eq!(message.right(), keyboard.x);
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| render_shell_chrome(frame, &layout, &model, &context))
                .unwrap();
            let status: String = message
                .positions()
                .map(|point| terminal.backend().buffer()[point].symbol())
                .collect();
            let expected = model.status.full_message();
            assert!(
                status
                    .trim_start()
                    .starts_with(expected.chars().next().unwrap())
            );
            assert_eq!(status.replace(' ', ""), expected.replace(' ', ""));
            assert!(row(&terminal, 0).starts_with("TundraUX"));
            assert!(row(&terminal, 0).contains("09:30"));
            let shown = layout.with_input_keyboard(Rect::new(0, 10, width, height - 10), 1_000);
            assert!(shown.workspace_taskbar.is_none());
        }
    }
}
