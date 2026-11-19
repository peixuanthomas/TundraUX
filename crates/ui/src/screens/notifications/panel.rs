use super::layout::wrap_notification_text;
use super::model::{NotificationTone, NotificationViewModel, notification_tone_prefix};
use crate::RenderContext;
use crate::components::{Button, Scrollbar};
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::widgets::{Block, Clear, Paragraph};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationPanelAction {
    Dismiss(u64),
    Open(u64),
    Clear(u64),
    Close,
}

use NotificationPanelAction as Action;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationCard {
    pub id: u64,
    pub tone: NotificationTone,
    pub source: String,
    pub title: Option<String>,
    pub message: String,
    pub open_log: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationPanelModel {
    pub cards: Vec<NotificationCard>,
    pub focus: Option<Action>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RowStyle {
    Status,
    Heading,
    Meta(NotificationTone),
    Card,
    Gap,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationPanelLayout {
    pub area: Rect,
    pub body: Rect,
    pub scrollbar: Option<Rect>,
    pub scroll_offset: usize,
    rows: Vec<(RowStyle, String)>,
    // Card actions keep their logical row even when it is outside the viewport.
    actions: Vec<(Action, Option<usize>, Rect)>,
}

impl NotificationPanelLayout {
    pub fn new(
        bounds: Rect,
        model: &NotificationViewModel,
        panel: &NotificationPanelModel,
    ) -> Self {
        let width = bounds.width.min(76);
        let x = bounds.x + (bounds.width - width) / 2;
        let y = bounds.y.saturating_add(1).min(bounds.bottom());
        let text_width = width.saturating_sub(6).max(1);
        let mut rows = Vec::new();
        let mut actions = Vec::new();
        for line in wrap_notification_text(&model.message, text_width) {
            rows.push((RowStyle::Status, line));
        }
        rows.push((RowStyle::Heading, i18n::tr!("notification-panel-title")));
        if panel.cards.is_empty() {
            rows.push((RowStyle::Gap, i18n::tr!("notification-panel-empty")));
        }
        for card in &panel.cards {
            let meta = format!("{}  {}", notification_tone_prefix(card.tone), card.source);
            let head = rows.len();
            for line in wrap_notification_text(&meta, text_width.saturating_sub(4).max(1)) {
                rows.push((RowStyle::Meta(card.tone), line));
            }
            actions.push((Action::Dismiss(card.id), Some(head), Rect::default()));
            for text in card.title.iter().chain(std::iter::once(&card.message)) {
                for line in wrap_notification_text(text, text_width) {
                    rows.push((RowStyle::Card, line));
                }
            }
            if card.open_log {
                actions.push((Action::Open(card.id), Some(rows.len()), Rect::default()));
                rows.push((RowStyle::Card, String::new()));
            }
            rows.push((RowStyle::Gap, String::new()));
        }

        let height = bounds
            .height
            .saturating_sub(2)
            .min(28)
            .min(u16::try_from(rows.len().saturating_add(2)).unwrap_or(u16::MAX));
        let area = Rect::new(x, y, width, height);
        let body = Rect::new(
            x.saturating_add(1),
            y.saturating_add(1),
            width.saturating_sub(2),
            height.saturating_sub(2),
        );
        let max_scroll = rows.len().saturating_sub(usize::from(body.height));
        let scroll_offset = model.scroll_offset.min(max_scroll);
        let scrollbar = (max_scroll > 0 && !body.is_empty())
            .then(|| Rect::new(area.right() - 1, body.y, 1, body.height));
        for (action, row, rect) in &mut actions {
            if let Some(pos) = row.and_then(|row| row.checked_sub(scroll_offset))
                && pos < usize::from(body.height)
            {
                let y = body.y + pos as u16;
                *rect = match action {
                    Action::Dismiss(_) => Rect::new(body.right().saturating_sub(4), y, 3, 1),
                    _ => Rect::new(
                        body.x + 1,
                        y,
                        label_width(&i18n::tr!("notification-panel-open-log")),
                        1,
                    ),
                }
                .intersection(body);
            }
        }
        let footer_y = area.bottom().saturating_sub(1);
        let close = model.actions.first().map_or("", |a| a.label.as_str());
        let close_width = label_width(close).min(body.width);
        let close_rect = Rect::new(
            body.right().saturating_sub(close_width),
            footer_y,
            close_width,
            1,
        )
        .intersection(area);
        if let Some(first) = panel.cards.first() {
            let width = label_width(&i18n::tr!("notification-panel-clear"))
                .min(close_rect.x.saturating_sub(body.x + 1));
            actions.push((
                Action::Clear(first.id),
                None,
                Rect::new(close_rect.x.saturating_sub(width + 1), footer_y, width, 1)
                    .intersection(area),
            ));
        }
        actions.push((Action::Close, None, close_rect));
        Self {
            area,
            body,
            rows,
            actions,
            scrollbar,
            scroll_offset,
        }
    }

    pub fn line_count(&self) -> usize {
        self.rows.len()
    }

    pub fn action_at(&self, point: (u16, u16)) -> Option<Action> {
        self.actions
            .iter()
            .find(|(_, _, area)| area.contains(point.into()))
            .map(|a| a.0)
    }

    pub fn action_area(&self, target: Action) -> Option<Rect> {
        self.actions
            .iter()
            .find(|a| a.0 == target && !a.2.is_empty())
            .map(|a| a.2)
    }

    pub fn move_focus(&self, current: Option<Action>, back: bool) -> (Action, usize) {
        let len = self.actions.len();
        let next = match self.actions.iter().position(|a| Some(a.0) == current) {
            Some(i) if back => (i + len - 1) % len,
            Some(i) => (i + 1) % len,
            None if back => len - 1,
            None => 0,
        };
        let (action, row, _) = self.actions[next];
        let offset = row.map_or(self.scroll_offset, |line| {
            self.scroll_offset
                .min(line)
                .max((line + 1).saturating_sub(usize::from(self.body.height)))
        });
        (action, offset)
    }

    pub(crate) fn render(
        &self,
        frame: &mut Frame<'_>,
        model: &NotificationViewModel,
        panel: &NotificationPanelModel,
        context: &RenderContext,
    ) {
        if self.area.is_empty() {
            return;
        }
        let colors = context.theme;
        frame.render_widget(Clear, self.area);
        frame.render_widget(
            Block::default().style(Style::default().bg(colors.surface)),
            self.area,
        );
        for (index, (kind, text)) in self
            .rows
            .iter()
            .skip(self.scroll_offset)
            .take(usize::from(self.body.height))
            .enumerate()
        {
            let row = Rect::new(self.body.x, self.body.y + index as u16, self.body.width, 1);
            let (fg, bg) = match kind {
                RowStyle::Status => (colors.text, colors.surface),
                RowStyle::Heading => (colors.accent, colors.surface),
                RowStyle::Meta(NotificationTone::Warning) => (colors.warning, colors.raised),
                RowStyle::Meta(_) => (colors.danger, colors.raised),
                RowStyle::Card => (colors.text, colors.raised),
                RowStyle::Gap => (colors.muted, colors.surface),
            };
            frame.render_widget(Block::default().style(Style::default().bg(bg)), row);
            frame.render_widget(
                Paragraph::new(text.as_str()).style(Style::default().fg(fg).bg(bg)),
                Rect::new(row.x + 1, row.y, row.width.saturating_sub(2), 1),
            );
        }
        if let Some(track) = self.scrollbar {
            Scrollbar::new(self.rows.len(), self.body.height.into(), self.scroll_offset)
                .render_frame(frame, track, context);
        }
        for (action, _, area) in &self.actions {
            if area.is_empty() {
                continue;
            }
            let (id, label, name) = match action {
                Action::Dismiss(id) => (
                    format!("dismiss-{id}"),
                    i18n::tr!("notification-panel-cross"),
                    Some(i18n::tr!("notification-panel-dismiss")),
                ),
                Action::Open(id) => (
                    format!("open-{id}"),
                    i18n::tr!("notification-panel-open-log"),
                    None,
                ),
                Action::Clear(_) => ("clear".into(), i18n::tr!("notification-panel-clear"), None),
                Action::Close => (
                    "close".into(),
                    model
                        .actions
                        .first()
                        .map(|a| a.label.clone())
                        .unwrap_or_default(),
                    None,
                ),
            };
            let mut button = Button::new(format!("notification.{}.action.{id}", model.id), label)
                .with_bracketed_label(false)
                .with_background(colors.raised)
                .with_focus_fill(Some(colors.accent_soft));
            if let Some(name) = name {
                button = button.with_capture_name(name);
            }
            button.set_focused(panel.focus == Some(*action));
            button.render_borderless_frame(frame, *area, &context.compatibility_theme());
        }
    }
}

fn label_width(label: &str) -> u16 {
    u16::try_from(Line::from(label).width() + 2).unwrap_or(u16::MAX)
}
