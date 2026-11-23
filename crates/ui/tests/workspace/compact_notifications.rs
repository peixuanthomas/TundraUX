use ratatui::{Terminal, backend::TestBackend, layout::Rect, widgets::Paragraph};
use ui::*;

fn chrome() -> ShellChromeViewModel {
    ShellChromeViewModel {
        build_mode: "test".into(),
        display_mode: HomeDisplayMode::Auth,
        terminal_size: (49, 11),
        back_button_hovered: false,
        help_available: false,
        title_focused: false,
        help_button_hovered: false,
        help_button_focused: false,
        back_label: Some("Esc".into()),
        status: StatusViewModel {
            status: "Compact status".into(),
            toast: Some("Compact toast".into()),
            error: Some("Compact alert".into()),
            alert_tone: NotificationTone::Error,
            time_button_label: None,
            time_button_selected: false,
        },
    }
}

fn first_line(terminal: &Terminal<TestBackend>, width: u16) -> String {
    (0..width)
        .map(|x| terminal.backend().buffer()[(x, 0)].symbol())
        .collect()
}

#[test]
fn compact_pages_keep_the_highest_priority_notification_above_their_content() {
    let clock = ClockViewModel::new("now");
    let management = ManagementViewModel::default();
    let logs = LogsViewModel::default();
    let theme = TundraTheme::default_dark();
    let context = RenderContext::from_theme(&theme, Default::default(), Default::default());
    let chrome = chrome();
    for page in [
        ScreenContent::Clock(&clock),
        ScreenContent::Management(&management),
        ScreenContent::Logs(&logs),
    ] {
        let mut terminal = Terminal::new(TestBackend::new(49, 11)).unwrap();
        terminal
            .draw(|frame| {
                let layout = ShellFrameLayout::new(frame.area(), None, &context)
                    .with_back_label(chrome.back_label.as_deref());
                page.render_content(frame, &layout, &context, None, None);
                page.render_overlay(frame, &layout, &context);
                render_shell_chrome(frame, &layout, &chrome, &context);
            })
            .unwrap();
        let line = first_line(&terminal, 49);
        assert!(line.starts_with("TundraUX"), "{line}");
        assert!(line.contains("[ERROR] Compact alert"), "{line}");
        assert!(!line.contains("Compact toast"));
        assert!(!line.contains("Compact status"));
    }
}

#[test]
fn compact_chrome_shows_toasts_and_preserves_page_header_without_notifications() {
    let theme = TundraTheme::default_dark();
    let context = RenderContext::from_theme(&theme, Default::default(), Default::default());
    let mut chrome = chrome();
    chrome.status.error = None;
    let mut terminal = Terminal::new(TestBackend::new(49, 11)).unwrap();
    for has_toast in [true, false] {
        if !has_toast {
            chrome.status.toast = None;
        }
        terminal
            .draw(|frame| {
                let area = frame.area();
                let layout = ShellFrameLayout::new(area, None, &context)
                    .with_back_label(chrome.back_label.as_deref());
                frame.render_widget(
                    Paragraph::new("Page header"),
                    Rect::new(layout.main.x, layout.main.y, layout.main.width, 1),
                );
                render_shell_chrome(frame, &layout, &chrome, &context);
            })
            .unwrap();
        let line = first_line(&terminal, 49);
        assert!(line.starts_with("TundraUX"), "{line}");
        assert_eq!(line.contains("Compact toast"), has_toast);
        assert!(line.contains("◀ Esc"));
        let content: String = (0..49)
            .map(|x| terminal.backend().buffer()[(x, 1)].symbol())
            .collect();
        assert!(content.starts_with("Page header"));
    }
}

#[test]
fn compact_notification_respects_empty_and_tiny_terminal_bounds() {
    let context = RenderContext::default();
    let chrome = chrome();
    for (width, height) in [(0, 0), (1, 1), (2, 2), (49, 1)] {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| {
                let layout = ShellFrameLayout::new(frame.area(), None, &context)
                    .with_back_label(chrome.back_label.as_deref());
                render_shell_chrome(frame, &layout, &chrome, &context);
            })
            .unwrap();
        if width > 0 && height > 0 {
            let line = first_line(&terminal, width);
            assert!(line.starts_with('T'), "{width}x{height}: {line}");
            if width >= 49 {
                assert!(line.contains('◀'), "{width}x{height}: {line}");
            }
        }
    }
}

#[test]
fn minimum_full_chrome_keeps_alert_in_the_header_with_title_and_controls() {
    let context = RenderContext::default();
    let mut chrome = chrome();
    chrome.terminal_size = (108, 20);
    chrome.status.time_button_label = Some("09:30".into());
    let layout = ShellFrameLayout::new(Rect::new(0, 0, 108, 20), Some("09:30"), &context)
        .with_help_button(true)
        .with_back_label(chrome.back_label.as_deref());
    let mut terminal = Terminal::new(TestBackend::new(108, 20)).unwrap();
    terminal
        .draw(|frame| render_shell_chrome(frame, &layout, &chrome, &context))
        .unwrap();
    let header = first_line(&terminal, 108);
    for text in ["TundraUX", "F1 Help", "09:30", "◀ Esc"] {
        assert!(header.contains(text), "{header}");
    }
    let status: String = layout
        .status_message
        .unwrap()
        .positions()
        .map(|point| terminal.backend().buffer()[point].symbol())
        .collect();
    assert!(
        status.trim_start().starts_with("[ERROR] Compact alert"),
        "{status}"
    );
    assert!(!status.contains("Compact toast"));
    assert_eq!(layout.main, Rect::new(0, 1, 108, 18));
}
