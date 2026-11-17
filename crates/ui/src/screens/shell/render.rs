use ratatui::Frame;
use ratatui::layout::{HorizontalAlignment, Rect};
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Clear, Paragraph, Wrap};

use super::centered_rect;
use super::{
    ExitConfirmViewModel, ShellChromeViewModel, ShellFrameLayout, ShellLayout,
    TimeSyncDialogViewModel,
};
use crate::components::{
    Button, DialogLayout, Key, Surface, terminal_width, truncate_to_terminal_width,
};
use crate::screens::notifications::notification_tone_style;
use crate::{RenderContext, TundraTheme};

fn compact_terminal_message() -> String {
    i18n::tr!(
        "ui-shell-minimum-terminal-size",
        width = super::MIN_SHELL_TERMINAL_WIDTH,
        height = super::MIN_SHELL_TERMINAL_HEIGHT
    )
}

pub fn render_exit_confirmation(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &ExitConfirmViewModel,
    theme: &TundraTheme,
) {
    let context = RenderContext::from_theme(theme, Default::default(), Default::default());
    render_exit_confirmation_contextual(frame, area, model, &context);
}

pub fn render_exit_confirmation_contextual(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &ExitConfirmViewModel,
    context: &RenderContext,
) {
    let mut notification = crate::NotificationViewModel::new(
        "shell.exit",
        crate::NotificationLevel::Modal,
        crate::NotificationTone::Warning,
        &model.title,
        &model.message,
        vec![
            crate::NotificationActionViewModel::new("exit", &model.confirm_label),
            crate::NotificationActionViewModel::new("restart", &model.restart_label),
            crate::NotificationActionViewModel::new("cancel", &model.cancel_label).cancel(true),
        ],
    );
    notification.stacked_actions = true;
    crate::screens::notifications::render_notification_overlay_context(
        frame,
        area,
        &notification,
        context,
    );
}

pub fn render_time_sync_failure_dialog(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &TimeSyncDialogViewModel,
    theme: &TundraTheme,
) {
    let context = RenderContext::from_theme(theme, Default::default(), Default::default());
    render_time_sync_failure_dialog_contextual(frame, area, model, &context);
}

pub fn render_time_sync_failure_dialog_contextual(
    frame: &mut Frame<'_>,
    area: Rect,
    model: &TimeSyncDialogViewModel,
    context: &RenderContext,
) {
    let dialog = centered_rect(area, area.width.min(34), area.height.min(5));
    let close = Key::Escape.label();
    let layout = DialogLayout::new(dialog, 0, Some(&close));
    layout.render_frame(frame, &i18n::tr!("ui-shell-time-sync"), context);
    frame.render_widget(
        Paragraph::new(model.message())
            .style(Style::default().fg(context.theme.danger))
            .wrap(Wrap { trim: true }),
        layout.body,
    );
    Button::new("shell.time-sync.close", close)
        .with_bracketed_label(false)
        .with_background(context.theme.surface)
        .render_borderless_frame(frame, layout.close, &DialogLayout::close_theme(context));
}

pub fn render_compact_home(
    frame: &mut Frame<'_>,
    area: Rect,
    chrome: &ShellChromeViewModel,
    theme: &TundraTheme,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let (notification, style) = status_presentation(&chrome.status, theme);
    if area.width <= 2 || area.height <= 2 {
        let notification = truncate_status_text(&notification, area.width);
        frame.render_widget(Clear, area);
        frame.render_widget(
            Paragraph::new(Line::styled(notification, style)).alignment(HorizontalAlignment::Left),
            area,
        );
        return;
    }

    let context = RenderContext::from_theme(theme, Default::default(), Default::default());
    let surface = Surface::new()
        .titled(i18n::tr!("ui-shell-title"))
        .bordered(true);
    let inner = surface.inner(area);
    surface.render_frame(frame, area, &context);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let notification = truncate_status_text(&notification, inner.width);
    frame.render_widget(
        Paragraph::new(Line::styled(notification, style)).alignment(HorizontalAlignment::Center),
        Rect::new(inner.x, inner.y, inner.width, 1),
    );

    if inner.height > 1 {
        let size_message = truncate_status_text(&compact_terminal_message(), inner.width);
        frame.render_widget(
            Paragraph::new(size_message)
                .style(theme.muted_style())
                .alignment(HorizontalAlignment::Center),
            Rect::new(inner.x, inner.y.saturating_add(1), inner.width, 1),
        );
    }
}

/// Draw the shell-owned surfaces once, after application overlays and before shell modals.
pub fn render_shell_chrome(
    frame: &mut Frame<'_>,
    layout: &ShellFrameLayout,
    chrome: &ShellChromeViewModel,
    context: &RenderContext,
) {
    render_shell_chrome_with_status_focus(frame, layout, chrome, context, false);
}

/// The compositor supplies the logical keyboard focus for the status message button.
pub fn render_shell_chrome_with_status_focus(
    frame: &mut Frame<'_>,
    layout: &ShellFrameLayout,
    chrome: &ShellChromeViewModel,
    context: &RenderContext,
    status_focused: bool,
) {
    if layout.bounds.is_empty() {
        return;
    }
    let theme = context.compatibility_theme();
    let top = match layout.shell {
        ShellLayout::Full { top, .. } => top,
        ShellLayout::Compact(_) => {
            Rect::new(layout.bounds.x, layout.bounds.y, layout.bounds.width, 1)
        }
    };
    Surface::new().render_frame(frame, top, context);
    if !layout.title.is_empty() {
        let (title, style) = if layout.is_compact()
            && (chrome.status.error.is_some() || chrome.status.toast.is_some())
        {
            status_presentation(&chrome.status, &theme)
        } else {
            (i18n::tr!("ui-shell-title"), theme.title_style())
        };
        frame.render_widget(
            Paragraph::new(Line::styled(
                truncate_status_text(&title, layout.title.width),
                style,
            )),
            layout.title,
        );
    }
    if let Some(area) = layout.back_button {
        let mut button = Button::new(
            "shell.back",
            format!(
                "{} {}",
                crate::assets::BACK_ICON.trim(),
                chrome.back_shortcut
            ),
        )
        .with_bracketed_label(false)
        .with_centered_label(true);
        button.state.hovered = chrome.back_button_hovered;
        button.render_borderless_frame(frame, area, &theme);
    }
    if let Some(area) = layout.help_button {
        let mut button = Button::new("shell.help", i18n::tr!("ui-shell-help-button"))
            .with_bracketed_label(false)
            .with_centered_label(true);
        button.state.hovered = chrome.help_button_hovered;
        button.state.focused = chrome.help_button_focused;
        button.render_borderless_frame(frame, area, &theme);
    }
    if let (Some(area), Some(label)) = (layout.tasks_button, &chrome.tasks_label) {
        let mut button = Button::new("shell.tasks", label)
            .with_bracketed_label(false)
            .with_centered_label(true);
        button.state.focused = chrome.tasks_button_focused;
        button.state.hovered = chrome.tasks_button_hovered;
        button.render_borderless_frame(frame, area, &theme);
    }
    if let ShellLayout::Full { bottom, .. } = layout.shell
        && !bottom.is_empty()
    {
        Surface::new().render_frame(frame, bottom, context);
    }
    if let Some(message) = layout.status_message {
        let (text, style) = status_presentation(&chrome.status, &theme);
        let mut message_theme = theme.clone();
        message_theme.foreground = style.fg.unwrap_or(theme.foreground);
        let mut button = Button::new("shell.status", truncate_status_text(&text, message.width))
            .with_bracketed_label(false)
            .with_label_alignment(HorizontalAlignment::Left);
        button.set_focused(status_focused);
        button.render_borderless_frame(frame, message, &message_theme);
    }
    if let (Some(label), Some(area)) = (&chrome.status.time_button_label, layout.time_button) {
        let mut button = Button::new("shell.status.time", label)
            .with_bracketed_label(false)
            .with_centered_label(true);
        button.state.selected = chrome.status.time_button_selected;
        button.render_borderless_frame(frame, area, &theme);
    }
}

fn status_presentation(status: &crate::StatusViewModel, theme: &TundraTheme) -> (String, Style) {
    if status.error.is_some() {
        return (
            status.full_message(),
            notification_tone_style(status.alert_tone, theme),
        );
    }
    if status.toast.is_some() {
        return (status.full_message(), theme.muted_style());
    }
    (status.full_message(), theme.body_style())
}

fn truncate_status_text(text: &str, width: u16) -> String {
    let text = text
        .chars()
        .map(|character| match character {
            '\r' | '\n' => ' ',
            character => character,
        })
        .collect::<String>();
    let width = usize::from(width);
    if terminal_width(&text) <= width {
        return text;
    }
    if width <= 3 {
        return truncate_to_terminal_width(&text, width);
    }

    let content_width = width.saturating_sub(3);
    let visible = truncate_to_terminal_width(&text, content_width);
    format!("{visible}...")
}

#[cfg(test)]
fn text_width(text: &str) -> u16 {
    u16::try_from(terminal_width(text)).unwrap_or(u16::MAX)
}

pub(crate) fn fit_cell(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }

    let text_width = terminal_width(text);
    if text_width <= width {
        let mut fitted = text.to_string();
        fitted.extend(std::iter::repeat_n(' ', width.saturating_sub(text_width)));
        return fitted;
    }

    let content_width = width.saturating_sub(1);
    let mut fitted = truncate_to_terminal_width(text, content_width);
    fitted.push('…');
    let used = terminal_width(&fitted);
    fitted.extend(std::iter::repeat_n(' ', width.saturating_sub(used)));
    fitted
}

#[cfg(test)]
#[path = "../../../tests/unit/screens/shell/render/tests.rs"]
mod tests;
