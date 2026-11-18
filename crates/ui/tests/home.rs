#[path = "support/composition.rs"]
mod composition;
use composition as ui;
mod support;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};
use support::terminal_output;
use ui::{
    ClockViewModel, ExitConfirmViewModel, HomeDisplayMode, HomeViewModel,
    NotificationActionViewModel, NotificationLayout, NotificationLevel, NotificationTone,
    NotificationViewModel, ShellChromeViewModel, ShellEntry, ShellLayout, StatusViewModel,
    TundraTheme, compute_shell_layout, notification_layout, notification_too_small_message,
    render_clock, render_exit_confirmation, render_home, render_notification_overlay,
};

#[test]
fn home_and_launcher_share_built_in_application_artwork_and_images() {
    let assets = ui::RuntimeAsciiAssets::load_default().unwrap();
    let home = HomeViewModel::user_with_icon_assets("User", "Now", Vec::new(), assets.clone());
    let items = app::BUILT_IN_APPLICATIONS
        .iter()
        .copied()
        .map(ui::LauncherItemViewModel::built_in)
        .collect();
    let launcher = ui::LauncherViewModel::with_ascii_assets(
        items,
        None,
        ui::LauncherViewMode::LargeIcons,
        false,
        assets,
    );
    for item in &launcher.items {
        let key = match item.id.as_str() {
            "builtin.system-status" => "system_status",
            id => id.strip_prefix("builtin.").unwrap(),
        };
        let application_key = match item.id.as_str() {
            "builtin.script-editor" => "builtin.editor",
            id => id,
        };
        let icon = home.home_icon_for_label(key).unwrap();
        assert_eq!(icon.key(), application_key);
        assert!(
            std::ptr::eq(icon, launcher.item_icon(item).unwrap()),
            "{key}"
        );
        assert_eq!(
            home.home_icon_image_path_for_label(key),
            launcher.item_graphic_path(item),
            "Home image path {key}"
        );
        assert_eq!(
            Some(home.home_icon_image_bytes_for_label(key).unwrap()),
            launcher.item_graphic_bytes(item),
            "Home image bytes {key}"
        );
    }
}

#[test]
fn home_cards_show_only_names_in_compact_and_full_layouts() {
    for labels in [
        [
            "Explorer",
            "Launcher",
            "Settings",
            "System Status",
            "Users",
            "Tasks",
        ],
        [
            "文件管理器",
            "启动器",
            "设置",
            "系统状态",
            "用户管理",
            "任务",
        ],
    ] {
        let entries = [
            "explorer",
            "launcher",
            "settings",
            "system_status",
            "users",
            "tasks",
        ]
        .into_iter()
        .zip(labels)
        .map(|(key, label)| ShellEntry::new(label, "").with_icon_key(key))
        .collect();
        let home = HomeViewModel::user("User", "Now", entries);
        for (width, height) in [(108, 8), (140, 40)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| {
                    ui::render_home_content(
                        frame,
                        frame.area(),
                        &home,
                        &ui::RenderContext::default(),
                        None,
                    )
                })
                .unwrap();
            let text = terminal_output(&terminal).replace(' ', "");
            for (shortcut, label) in ['E', 'A', 'S', 'M', 'U', 'T'].into_iter().zip(labels) {
                let label = label.replace(' ', "");
                assert!(text.contains(&label), "missing {label} at {width}x{height}");
                assert!(
                    !text.contains(&format!("{shortcut}{label}")),
                    "unexpected shortcut for {label}"
                );
            }
        }
    }
}

#[test]
fn short_home_cards_show_names_without_clipping_ascii_or_image_icons_after_resize() {
    struct ImageRenderer(std::cell::Cell<usize>);
    impl ui::HomeIconRenderer for ImageRenderer {
        fn render_icon(&self, _: &str, frame: &mut ratatui::Frame<'_>, area: Rect) -> bool {
            self.0.set(self.0.get() + 1);
            frame.render_widget(ratatui::widgets::Paragraph::new("IMAGE"), area);
            true
        }
    }

    for labels in [["Explorer", "Settings"], ["文件管理器", "设置"]] {
        let home = HomeViewModel::user(
            "User",
            "Now",
            ["explorer", "settings"]
                .into_iter()
                .zip(labels)
                .map(|(key, label)| ShellEntry::new(label, "DESCRIPTION").with_icon_key(key))
                .collect(),
        );
        for graphical in [false, true] {
            let images = ImageRenderer(std::cell::Cell::new(0));
            let mut terminal = Terminal::new(TestBackend::new(108, 9)).unwrap();
            for tile_height in [6, 5, 4, 3, 2, 1, 5, 6] {
                let height = tile_height + 3;
                terminal.backend_mut().resize(108, height);
                let main = Rect::new(0, 0, 108, height);
                let layout = ui::home_layout(main, &home);
                assert_eq!(layout.items.len(), home.entries().len());
                let calls = images.0.get();
                terminal
                    .draw(|frame| {
                        ui::render_home_content(
                            frame,
                            main,
                            &home,
                            &ui::RenderContext::default(),
                            graphical.then_some(&images as &dyn ui::HomeIconRenderer),
                        )
                    })
                    .unwrap();
                let expected_calls = if graphical && tile_height >= 5 {
                    calls + home.entries().len()
                } else {
                    calls
                };
                assert_eq!(images.0.get(), expected_calls);
                for item in &layout.items {
                    let tile = item.area;
                    let entry = &home.entries()[item.index];
                    assert_eq!(tile.height, tile_height.min(5));
                    let text = (tile.y..tile.bottom())
                        .map(|y| buffer_row_text(&terminal, tile.x, y, tile.width))
                        .collect::<Vec<_>>()
                        .join("\n");
                    assert!(text.replace(' ', "").contains(&entry.label));
                    assert!(!text.contains("DESCRIPTION"));
                    let buffer = terminal.backend().buffer();
                    let underlined: String = tile
                        .positions()
                        .filter_map(|pos| {
                            let cell = &buffer[pos];
                            cell.modifier
                                .contains(Modifier::UNDERLINED)
                                .then_some(cell.symbol())
                        })
                        .collect();
                    assert_eq!(
                        underlined,
                        if item.index == home.selected_entry_index() {
                            &entry.label
                        } else {
                            ""
                        },
                        "tile height {tile_height}, graphical icons {graphical}"
                    );
                    let icon = home.home_icon_for_label(entry.icon_identity()).unwrap();
                    if tile_height < 5 {
                        assert!(ui::home_entry_icon_area(tile).is_empty());
                        assert!(!text.contains("IMAGE"));
                        let first_icon_line = icon
                            .lines()
                            .iter()
                            .find(|line| !line.trim().is_empty())
                            .unwrap();
                        assert!(!text.contains(first_icon_line.trim()));
                    } else {
                        assert_eq!(ui::home_entry_icon_area(tile).height, 4);
                        if graphical {
                            assert!(text.contains("IMAGE"));
                        } else {
                            for line in icon.lines() {
                                assert!(text.contains(line.trim()));
                            }
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn home_entry_index_at_maps_coordinates_to_entry_tiles() {
    let main = main_rect(100, 30);
    let tile_areas = ui::home_entry_tile_areas(main, 5);
    let first_tile = tile_areas[0];
    let second_tile = tile_areas[1];

    assert_eq!(
        ui::home_entry_index_at(
            main,
            5,
            (
                first_tile.x.saturating_add(1),
                first_tile.y.saturating_add(1),
            ),
        ),
        Some(0)
    );
    assert_eq!(
        ui::home_entry_index_at(
            main,
            5,
            (
                second_tile.x.saturating_add(1),
                second_tile.y.saturating_add(1),
            ),
        ),
        Some(1)
    );
    assert_eq!(ui::home_entry_index_at(main, 5, (main.x, main.y)), None);
}

#[test]
fn home_scrolling_keeps_tail_entries_and_uses_the_freed_footer_space() {
    let entries = (0..30)
        .map(|index| ShellEntry::new(format!("App {index}"), "Open this app"))
        .collect();
    let home =
        HomeViewModel::user_with_selection("User", "Now", entries, 29).with_viewport_offset(25);
    let main = Rect::new(3, 2, 32, 20);
    let layout = ui::home_layout(main, &home);
    assert!(layout.scrollbar.is_some());
    assert!(layout.items.iter().any(|item| item.index == 29));
    assert_eq!(layout.scrollbar.unwrap().bottom(), main.bottom() - 1);
    for item in &layout.items {
        assert!(item.area.x >= main.x && item.area.right() <= main.right());
        assert!(item.area.y >= main.y && item.area.bottom() <= main.bottom());
        assert_eq!(
            layout.entry_at((item.area.x, item.area.y)),
            Some(item.index)
        );
    }
}

#[test]
fn compact_home_and_clock_show_highest_priority_notification() {
    let theme = TundraTheme::default_dark();
    let mut outputs = Vec::new();

    let home = HomeViewModel::user("Strix", "now", Vec::new());
    let chrome = compact_alert_chrome();
    let mut terminal = Terminal::new(TestBackend::new(49, 11)).expect("test terminal");
    terminal
        .draw(|frame| render_home(frame, frame.area(), &chrome, &home, &theme))
        .expect("render compact home");
    outputs.push(terminal_output(&terminal));

    let clock = ClockViewModel::new("now");
    let chrome = compact_alert_chrome();
    let mut terminal = Terminal::new(TestBackend::new(49, 11)).expect("test terminal");
    terminal
        .draw(|frame| render_clock(frame, frame.area(), &chrome, &clock, &theme))
        .expect("render compact clock");
    outputs.push(terminal_output(&terminal));

    for output in outputs {
        assert!(output.contains("[ERROR] Compact alert"));
        assert!(!output.contains("Compact toast"));
        assert!(!output.contains("Compact status"));
    }
}

#[test]
fn extremely_small_compact_layout_keeps_escape_above_borderless_notification() {
    let home = HomeViewModel::user("User", "Now", Vec::new());
    let chrome = compact_alert_chrome();
    let mut terminal = Terminal::new(TestBackend::new(2, 2)).expect("test terminal");

    terminal
        .draw(|frame| {
            render_home(
                frame,
                frame.area(),
                &chrome,
                &home,
                &TundraTheme::default_dark(),
            );
        })
        .expect("render tiny compact notification");

    assert!(buffer_row_text(&terminal, 0, 0, 2).contains('◀'));
    assert_eq!(buffer_row_text(&terminal, 0, 1, 2), "[E");
}

#[test]
fn exit_menu_actions_remain_separate_and_visible_after_resize() {
    let labels = [
        "Exit TundraUX",
        "Restart TundraUX",
        "Restart computer",
        "Shut down computer",
        "Cancel",
    ];
    let mut model = NotificationViewModel::new(
        "exit",
        NotificationLevel::Modal,
        NotificationTone::Warning,
        "Exit & power",
        "Choose an action. Esc returns to TundraUX.",
        labels
            .iter()
            .zip(["Y", "R", "B", "P", "N"])
            .map(|(label, key)| NotificationActionViewModel::new(*label, *label).with_shortcut(key))
            .collect(),
    );
    model.stacked_actions = true;
    for (width, height) in [(120, 40), (80, 24), (50, 12)] {
        let area = Rect::new(0, 0, width, height);
        let NotificationLayout::Dialog(layout) = notification_layout(area, &model) else {
            panic!("exit actions must fit {width}x{height}");
        };
        assert_eq!(layout.actions.len(), labels.len());
        for pair in layout.actions.windows(2) {
            assert_eq!(pair[0].area.x, pair[1].area.x);
            assert_eq!(pair[0].area.width, pair[1].area.width);
            assert!(pair[0].area.bottom() <= pair[1].area.y);
        }
        assert!(
            layout
                .actions
                .iter()
                .all(|action| action.area.bottom() < layout.dialog.bottom())
        );
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| {
                render_notification_overlay(frame, area, &model, &TundraTheme::default_dark())
            })
            .unwrap();
        let output = terminal_output(&terminal);
        for label in labels {
            assert!(output.contains(label), "{width}x{height}: {label}");
        }
    }
}

#[test]
fn exit_confirmation_keeps_all_componentized_actions_visible() {
    let model = ExitConfirmViewModel::new();
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");

    terminal
        .draw(|frame| {
            render_exit_confirmation(frame, frame.area(), &model, &TundraTheme::default_dark());
        })
        .expect("render exit confirmation");

    let output = terminal_output(&terminal);
    for text in [
        &model.title,
        &model.message,
        &model.confirm_label,
        &model.restart_label,
        &model.cancel_label,
    ] {
        assert!(output.contains(text));
    }
}

#[test]
fn notification_overlay_renders_modal_actions_and_replaces_too_small_terminal_content() {
    let model = NotificationViewModel::new(
        "42",
        NotificationLevel::Modal,
        NotificationTone::Warning,
        "Delete File",
        "Move README.md to TundraUX trash?",
        vec![
            NotificationActionViewModel::new("confirm", "Move")
                .with_shortcut("Y")
                .selected(true),
            NotificationActionViewModel::new("cancel", "Cancel").with_shortcut("N"),
        ],
    );
    let theme = TundraTheme::default_dark();

    let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");
    terminal
        .draw(|frame| render_notification_overlay(frame, frame.area(), &model, &theme))
        .expect("render notification");

    let output = terminal_output(&terminal);
    assert!(output.contains("[WARN] Delete File"));
    assert!(output.contains("Delete File"));
    assert!(output.contains("Move README.md to TundraUX trash?"));
    assert!(output.contains("Move Y"));
    assert!(output.contains("Cancel N"));
    let NotificationLayout::Dialog(layout) =
        notification_layout(terminal.backend().buffer().area, &model)
    else {
        panic!("notification dialog should fit");
    };
    assert!(layout.actions[1].area.y < layout.message.y);
    assert!(layout.actions[0].area.y > layout.message.y);

    let mut narrow = Terminal::new(TestBackend::new(18, 10)).expect("test terminal");
    narrow
        .draw(|frame| {
            frame.render_widget(
                ratatui::widgets::Paragraph::new("BACKGROUND CONTENT"),
                frame.area(),
            );
            render_notification_overlay(frame, frame.area(), &model, &theme);
        })
        .expect("render narrow notification");

    let narrow_output = terminal_output(&narrow);
    assert!(
        visible_text_without_spaces(&narrow_output).contains(&visible_text_without_spaces(
            &notification_too_small_message()
        ))
    );
    assert!(!narrow_output.contains("BACKGROUND CONTENT"));
    assert!(!narrow_output.contains("Delete File"));
    assert!(!narrow_output.contains("README.md"));
    assert!(!narrow_output.contains("Move Y"));
    assert!(!narrow_output.contains("Cancel N"));
}

#[test]
fn notification_wrapping_uses_terminal_columns_for_wide_text() {
    let model = NotificationViewModel::new(
        "wide",
        NotificationLevel::Modal,
        NotificationTone::Info,
        "Wide text",
        "界".repeat(20),
        Vec::new(),
    );

    let NotificationLayout::Dialog(layout) = notification_layout(Rect::new(0, 0, 40, 12), &model)
    else {
        panic!("wide notification should fit");
    };

    assert_eq!(layout.message.width, 34);
    assert_eq!(layout.message.height, 2);
}

#[test]
fn notification_layout_and_renderer_share_wrapped_message_and_stacked_action_rects() {
    let model = NotificationViewModel::new(
        "long",
        NotificationLevel::Modal,
        NotificationTone::Info,
        "Long Notification",
        "M".repeat(190),
        vec![
            NotificationActionViewModel::new("first", "A".repeat(70)).selected(true),
            NotificationActionViewModel::new("second", "B".repeat(40)),
        ],
    );
    let area = Rect::new(3, 4, 80, 20);
    let NotificationLayout::Dialog(layout) = notification_layout(area, &model) else {
        panic!("long notification should fit the supplied terminal");
    };

    assert_eq!(layout.dialog.x, 11);
    assert_eq!(layout.dialog.width, 64);
    assert_eq!(layout.message.x - layout.dialog.x, 3);
    assert_eq!(layout.message.width, 58);
    assert_eq!(layout.message.height, 4);
    assert_eq!(layout.actions.len(), 2);
    assert_eq!(layout.actions[0].area.height, 2);
    assert_eq!(layout.actions[1].area.height, 1);
    for action in &layout.actions {
        assert_eq!(layout.dialog.right() - action.area.right(), 3);
        assert!(action.area.y >= layout.message.bottom());
    }
    let NotificationLayout::Dialog(scrolled) = notification_layout(Rect::new(0, 0, 64, 9), &model)
    else {
        panic!("long messages should scroll while their actions remain visible");
    };
    assert!(scrolled.message.height < layout.message.height);
    assert!(scrolled.max_scroll_offset > 0);
    assert!(
        scrolled
            .actions
            .iter()
            .all(|action| action.area.intersection(scrolled.dialog) == action.area)
    );

    let mut terminal = Terminal::new(TestBackend::new(86, 28)).expect("test terminal");
    let mut theme = TundraTheme::default_dark();
    theme.buttons = Some(ui::components::ButtonFrame::new(None, None, &theme));
    terminal
        .draw(|frame| render_notification_overlay(frame, area, &model, &theme))
        .expect("render long notification");
    let buttons = theme.buttons.as_ref().unwrap().regions();
    for action in &layout.actions {
        let label = &model.actions[action.index];
        let text = action
            .area
            .positions()
            .map(|point| terminal.backend().buffer()[point].symbol())
            .collect::<String>()
            .replace(' ', "");
        assert_eq!(text, label.label);
        let id = format!("notification.long.action.{}", label.id);
        let button = buttons
            .iter()
            .find(|button| button.id.as_str() == id)
            .unwrap();
        assert_eq!(button.area, action.area);
        assert!(!button.disabled);
    }
    assert!(region_has_bg(
        &terminal,
        layout.actions[0].area,
        TundraTheme::default_dark().tokens().accent_soft,
    ));
    assert!(region_has_fg(
        &terminal,
        layout.actions[0].area,
        TundraTheme::default_dark().accent_color,
    ));
    assert!(region_has_fg(
        &terminal,
        layout.actions[1].area,
        TundraTheme::default_dark().foreground,
    ));
}

fn chrome() -> ShellChromeViewModel {
    ShellChromeViewModel {
        build_mode: "debug".to_string(),
        display_mode: HomeDisplayMode::Auth,
        terminal_size: (80, 24),
        back_button_hovered: false,
        help_available: false,
        tasks_label: None,
        tasks_button_focused: false,
        tasks_button_hovered: false,
        help_button_hovered: false,
        help_button_focused: false,
        back_shortcut: "Esc",
        status: StatusViewModel {
            status: "Ready".to_string(),
            toast: None,
            error: None,
            alert_tone: NotificationTone::Info,
            time_button_label: None,
            time_button_selected: false,
        },
    }
}

fn compact_alert_chrome() -> ShellChromeViewModel {
    let mut chrome = chrome();
    chrome.terminal_size = (49, 11);
    chrome.status = StatusViewModel {
        status: "Compact status".to_string(),
        toast: Some("Compact toast".to_string()),
        error: Some("Compact alert".to_string()),
        alert_tone: NotificationTone::Error,
        time_button_label: None,
        time_button_selected: false,
    };
    chrome
}

fn visible_text_without_spaces(output: &str) -> String {
    output
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect()
}

fn buffer_row_text(terminal: &Terminal<TestBackend>, x: u16, y: u16, width: u16) -> String {
    let buffer = terminal.backend().buffer();
    (x..x.saturating_add(width))
        .filter_map(|column| buffer.cell((column, y)))
        .map(|cell| cell.symbol())
        .collect()
}

fn main_rect(width: u16, height: u16) -> Rect {
    match compute_shell_layout(Rect::new(0, 0, width, height)) {
        ShellLayout::Full { main, .. } => main,
        ShellLayout::Compact(_) => panic!("home render tests expect a full shell layout"),
    }
}

fn region_has_fg(terminal: &Terminal<TestBackend>, area: Rect, fg: Color) -> bool {
    let buffer = terminal.backend().buffer();
    (area.y..area.y.saturating_add(area.height)).any(|y| {
        (area.x..area.x.saturating_add(area.width)).any(|x| {
            buffer
                .cell((x, y))
                .is_some_and(|cell| cell.fg == fg && cell.symbol() != " ")
        })
    })
}

fn region_has_bg(terminal: &Terminal<TestBackend>, area: Rect, bg: Color) -> bool {
    let buffer = terminal.backend().buffer();
    (area.y..area.y.saturating_add(area.height)).any(|y| {
        (area.x..area.x.saturating_add(area.width)).any(|x| {
            buffer
                .cell((x, y))
                .is_some_and(|cell| cell.bg == bg && cell.symbol() != " ")
        })
    })
}
