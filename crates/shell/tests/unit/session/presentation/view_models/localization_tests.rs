use super::*;

fn snapshots() -> [std::sync::Arc<i18n::LanguageSnapshot>; 2] {
    let root = std::env::temp_dir().join(format!(
        "tux-shell-message-tests-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let canonical =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ascii-assets/assets/locales");
    for code in ["en-US", "zh-CN"] {
        let locale = root.join("locales").join(code);
        std::fs::create_dir_all(locale.join("modules")).unwrap();
        for relative in ["manifest.toml", "modules/shell-messages.ftl"] {
            std::fs::copy(canonical.join(code).join(relative), locale.join(relative)).unwrap();
        }
    }
    let snapshots = ["en-US", "zh-CN"].map(|code| {
        std::sync::Arc::new(
            i18n::LanguageSnapshot::load(&root, code, 1)
                .unwrap()
                .snapshot,
        )
    });
    // Formatting must work entirely from the snapshots, including nested messages.
    std::fs::remove_dir_all(root).unwrap();
    snapshots
}

#[test]
fn retained_shell_notifications_render_with_the_session_snapshot() {
    let [english, chinese] = snapshots();
    let mut session = ShellSession::new(ShellLaunchConfig::default(), (120, 30));
    session.home_mode = ShellHomeMode::User;
    session.language = english.clone();
    let raw_path = "/tmp/{ready}/Saved.txt";
    session.notify_status(i18n::msg!("shell-saving-arg1", arg1 = raw_path));
    session.notify_toast(i18n::msg!(
        "shell-home-arg1",
        arg1 = i18n::msg!("shell-explorer")
    ));
    let retained_status = session.app.notification_center().status().clone();
    let english_model = session.to_shell_chrome_view_model();
    assert_eq!(english_model.status.status, format!("Saving {raw_path}"));
    assert_eq!(
        english_model.status.toast.as_deref(),
        Some("Home: Explorer")
    );

    // The public VM entry must override a different ambient snapshot and restore it on exit.
    let _ambient = i18n::enter_snapshot(english);
    session.language = chinese;
    let chinese_model = session.to_shell_chrome_view_model();
    assert_eq!(chinese_model.status.status, format!("正在保存 {raw_path}"));
    assert_eq!(
        chinese_model.status.toast.as_deref(),
        Some("主页：文件管理器")
    );
    assert_eq!(session.app.notification_center().status(), &retained_status);
    assert_eq!(i18n::tr!("shell-ready"), "Ready");
}

#[test]
fn translated_explorer_sort_choices_keep_stable_command_ids() {
    let [_, chinese] = snapshots();
    let _language = i18n::enter_snapshot(chinese);
    let ui::ExplorerOverlayViewModel::ContextMenu(menu) =
        explorer_sort_menu_view_model((0, 0), ui::ExplorerSortColumn::Name, 0)
    else {
        panic!("sort menu expected");
    };
    assert_eq!(menu.title, "排序方式");
    assert_eq!(
        menu.items
            .iter()
            .map(|item| item.id.as_str())
            .collect::<Vec<_>>(),
        ["sort-name", "sort-type", "sort-size", "sort-modified"]
    );
}

#[test]
fn selected_file_menu_keeps_the_actions_moved_from_the_toolbar() {
    let ui::ExplorerOverlayViewModel::ContextMenu(menu) =
        explorer_context_menu_view_model(ExplorerContextMenuInput {
            anchor: (0, 0),
            centered: true,
            selected_count: 1,
            entry_count: 1,
            clipboard_available: false,
            undo_available: false,
            archive_busy: false,
            selected_archive: false,
            is_trash: false,
            trash_has_items: false,
            focused_index: 0,
            can_manage_launcher: false,
            can_execute_command_line: false,
            launcher_eligible_count: 0,
        })
    else {
        panic!("file menu");
    };
    for id in ["cut", "copy", "paste", "sort", "options"] {
        assert!(menu.items.iter().any(|item| item.id == id), "{id}");
    }
    assert!(
        !menu
            .items
            .iter()
            .find(|item| item.id == "paste")
            .unwrap()
            .enabled
    );
}

#[test]
fn localized_status_stays_above_the_command_line_viewport() {
    use ratatui::{Terminal, backend::TestBackend};

    let assets = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ascii-assets/assets");
    for (code, expected) in [
        ("en-US", "Saving sample.txt"),
        ("zh-CN", "正在保存 sample.txt"),
    ] {
        let mut state = ShellSession::new_for_home_mode(
            ShellLaunchConfig::default(),
            (108, 20),
            ShellHomeMode::User,
        );
        state.language = std::sync::Arc::new(
            i18n::LanguageSnapshot::load(&assets, code, 1)
                .unwrap()
                .snapshot,
        );
        let _language = i18n::enter_snapshot(state.language.clone());
        state.set_navigation_path(vec![ShellScreen::Home, ShellScreen::CommandLine]);
        state.notify_status(i18n::msg!("shell-saving-arg1", arg1 = "sample.txt"));
        let chrome = state.to_shell_chrome_view_model();
        let context = ui::RenderContext::default();
        let layout = ui::ShellFrameLayout::new(
            Rect::new(0, 0, 108, 20),
            chrome.status.time_button_label.as_deref(),
            &context,
        )
        .with_back_button(true, None);
        assert_eq!(
            ui::command_line_terminal_area_in(&layout),
            Some(Rect::new(0, 1, 108, 18))
        );

        let mut snapshot = ui::CommandLineTerminalSnapshot::blank(108, 18);
        for (index, character) in "PTY LAST ROW".chars().enumerate() {
            snapshot.cells[17 * 108 + index].symbol = character.to_string();
        }
        let command_line = ui::CommandLineViewModel::new(snapshot);
        let mut terminal = Terminal::new(TestBackend::new(108, 20)).unwrap();
        terminal
            .draw(|frame| {
                ui::ScreenContent::CommandLine(&command_line)
                    .render_content(frame, &layout, &context, None, None);
                ui::render_shell_chrome(frame, &layout, &chrome, &context);
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let message = layout.status_message.unwrap();
        assert_eq!(message.y, 0);
        let status: String = message
            .positions()
            .map(|point| buffer[point].symbol())
            .collect();
        assert!(
            status
                .replace(' ', "")
                .starts_with(&expected.replace(' ', "")),
            "{code}: {status}"
        );
        let last_row: String = (0..108).map(|x| buffer[(x, 18)].symbol()).collect();
        assert!(last_row.starts_with("PTY LAST ROW"), "{code}: {last_row}");
    }
}
