use super::{fit_cell, text_width, truncate_status_text};
use ratatui::text::Line;

#[test]
fn status_message_uses_shared_hover_pressed_and_keyboard_colors() {
    use crate::components::{ButtonFrame, ButtonRegion};
    use crate::{
        HomeDisplayMode, RenderContext, ShellChromeViewModel, ShellFrameLayout, StatusViewModel,
        TundraTheme,
    };
    use ratatui::{
        Terminal,
        backend::TestBackend,
        layout::Rect,
        style::{Color, Modifier},
    };
    let mut theme = TundraTheme::default_dark();
    theme.accent_color = Color::Rgb(30, 120, 200);
    let context = RenderContext::from_theme(&theme, Default::default(), Default::default());
    let layout = ShellFrameLayout::new(Rect::new(0, 0, 80, 24), Some("12:00"), &context);
    let message = layout.status_message.unwrap();
    let region = ButtonRegion {
        id: "shell.status".into(),
        area: message,
        disabled: false,
    };
    let chrome = ShellChromeViewModel {
        build_mode: "debug".into(),
        display_mode: HomeDisplayMode::User,
        terminal_size: (80, 24),
        back_button_hovered: false,
        help_available: false,
        title_focused: false,
        help_button_hovered: false,
        help_button_focused: false,
        back_label: Some("Esc".into()),
        status: StatusViewModel {
            status: "Complete status message".into(),
            toast: None,
            error: None,
            alert_tone: crate::NotificationTone::Info,
            time_button_label: Some("12:00".into()),
            time_button_selected: false,
        },
    };
    for (hovered, pressed, keyboard, focused, expected) in [
        (true, false, false, false, Color::White),
        (true, true, false, false, Color::White),
        (false, false, true, true, theme.accent_color),
        (false, false, false, false, theme.foreground),
    ] {
        let mut buttons = ButtonFrame::new(
            hovered.then(|| region.clone()),
            pressed.then(|| region.clone()),
            &theme,
        );
        buttons.keyboard_focus_visible = keyboard;
        let mut context = context.clone();
        context.buttons = Some(buttons.clone());
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|frame| {
                super::render_shell_chrome_with_status_focus(
                    frame, &layout, &chrome, &context, focused,
                )
            })
            .unwrap();
        let buffer = terminal.backend().buffer();
        let label = message
            .positions()
            .map(|position| &buffer[position])
            .find(|cell| cell.symbol() == "C")
            .unwrap();
        assert_eq!(label.fg, expected);
        assert_eq!(label.modifier.contains(Modifier::UNDERLINED), focused);
        if hovered {
            let background_theme = TundraTheme {
                background: label.bg,
                ..theme.clone()
            };
            assert_eq!(background_theme.readable_text_color(label.fg), label.fg);
        }
        assert!(buttons.regions().contains(&region));
        assert_ne!(layout.time_button.unwrap(), message);
    }
    let mut chrome = chrome;
    chrome.status.status = "Ready".into();
    for visibility in [100, 500] {
        let layout = layout.with_input_keyboard(Rect::new(0, 24, 80, 0), visibility);
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|frame| super::render_shell_chrome(frame, &layout, &chrome, &context))
            .unwrap();
        assert_eq!(layout.status_message, Some(message));
        assert!(layout.time_button.is_some());
        assert_eq!(layout.main.bottom(), 24);
    }
}

#[test]
fn clock_sits_between_help_and_back_and_help_hides_in_compact() {
    use crate::{RenderContext, ShellFrameLayout};
    use ratatui::layout::Rect;
    let context = RenderContext::default();
    let layout = ShellFrameLayout::new(Rect::new(0, 0, 80, 24), Some("12:00"), &context)
        .with_back_label(Some("Esc"))
        .with_help_button(true);
    let back = layout.back_button.unwrap();
    let help = layout.help_button.unwrap();
    let time = layout.time_button.unwrap();
    assert_eq!(help.right() + 1, time.x);
    assert_eq!(time.right() + 1, back.x);
    assert_eq!(help.y, time.y);
    assert_eq!(time.y, back.y);
    let compact =
        ShellFrameLayout::new(Rect::new(0, 0, 40, 10), None, &context).with_help_button(true);
    assert!(compact.help_button.is_none());
}

#[test]
fn cell_fitting_and_status_truncation_use_terminal_display_width() {
    assert_eq!(text_width("界面"), 4);
    assert_eq!(fit_cell("界面", 5), "界面 ");
    assert_eq!(fit_cell("界面", 3), "界…");
    assert_eq!(Line::from(fit_cell("界面", 3)).width(), 3);
    assert_eq!(truncate_status_text("界面状态", 5), "界...");
    assert_eq!(Line::from(truncate_status_text("界面状态", 5)).width(), 5);
}
