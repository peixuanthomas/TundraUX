use std::sync::Arc;

use app::update::UpdateGitRow;
use ratatui::{
    Frame,
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};

use super::{SettingsField, SettingsFocus, SettingsItemViewModel, SettingsViewModel};
use crate::RenderContext;
use crate::components::{
    Button, ButtonEmphasis, Scrollbar, SidebarWorkspaceLayout, Surface, UpdateActivity,
    UpdateActivityViewModel, terminal_width, visible_scrolled_rect,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsUpdateConfirmationViewModel {
    pub title: String,
    pub body: String,
    pub confirm_label: String,
    pub confirm_selected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsUpdateViewModel {
    pub panel: SettingsUpdatePanel,
    pub action: Option<SettingsItemViewModel>,
    pub confirmation: Option<SettingsUpdateConfirmationViewModel>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsUpdatePanel {
    Idle,
    Release {
        notes: String,
        activity: UpdateActivityViewModel,
    },
    Source {
        rows: Arc<[UpdateGitRow]>,
        message: String,
        selected: Option<usize>,
        offset: usize,
        selectable: bool,
        activity: UpdateActivityViewModel,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsUpdateGraphLayout {
    pub rows: Rect,
    pub scrollbar: Option<Rect>,
    pub offset: usize,
    pub capacity: usize,
    pub total: usize,
    title: Option<Rect>,
    first_visible: usize,
    targets: Vec<(usize, Rect)>,
}

impl SettingsUpdateGraphLayout {
    pub(super) fn commit_at(&self, point: (u16, u16)) -> Option<usize> {
        self.targets
            .iter()
            .find_map(|(index, area)| area.contains(point.into()).then_some(*index))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SettingsUpdateLayout {
    pub graph: Option<SettingsUpdateGraphLayout>,
    pub(super) end_row: u16,
    pub(super) action_rows: Option<(u16, u16)>,
    pub(super) action: Option<Rect>,
    activity: Option<(Rect, i32, u16)>,
    notes: Option<(i32, u16)>,
}

impl SettingsUpdateLayout {
    pub(super) fn new(
        clip: Rect,
        offset: u16,
        mut row: u16,
        update: &SettingsUpdateViewModel,
    ) -> Self {
        let mut layout = Self::default();
        let top = |row| i32::from(clip.y) + i32::from(row) - i32::from(offset);
        let project =
            |area: Rect| visible_scrolled_rect(area.x, top(area.y), area.width, area.height, clip);
        let place_action = |layout: &mut Self, at: u16, height| {
            if let Some(action) = &update.action {
                let width = (terminal_width(&action.label) + 6)
                    .max(if height > 1 { 32 } else { 24 })
                    .min(usize::from(clip.width)) as u16;
                layout.action_rows = Some((at, height));
                layout.action = project(Rect::new(clip.right() - width, at, width, height))
                    .map(|(area, _)| area);
            }
        };
        match &update.panel {
            SettingsUpdatePanel::Idle => return layout,
            SettingsUpdatePanel::Release { notes, .. } => {
                place_action(&mut layout, row, 3);
                row += 4;
                layout.activity = Some((clip, top(row), 2));
                row += 3;
                let height = (note_lines(notes, clip.width).len() + 1)
                    .min(usize::from(u16::MAX - row)) as u16;
                layout.notes = Some((top(row), height));
                row += height;
            }
            SettingsUpdatePanel::Source {
                rows,
                offset,
                selectable,
                ..
            } => {
                let action_height = u16::from(update.action.is_some()) * 2;
                let available = clip.height.saturating_sub(row + action_height);
                let minimum = if clip.width >= 74 { 9 } else { 13 };
                let height = available.max(minimum);
                let (graph, build) = SidebarWorkspaceLayout::panes(
                    Rect::new(clip.x, row, clip.width, height),
                    false,
                );
                let capacity = usize::from(graph.height.saturating_sub(1));
                let total = rows.len();
                let offset = (*offset).min(total.saturating_sub(capacity));
                let track = (total > capacity && graph.width > 1)
                    .then(|| Rect::new(graph.right() - 1, graph.y + 1, 1, capacity as u16));
                let row_area = Rect::new(
                    graph.x,
                    graph.y + 1,
                    graph.width.saturating_sub(u16::from(track.is_some()) * 2),
                    capacity as u16,
                );
                let (visible, skipped) = project(row_area).unwrap_or_default();
                let first_visible = offset + usize::from(skipped);
                let targets = if *selectable {
                    rows.iter()
                        .enumerate()
                        .skip(first_visible)
                        .take(usize::from(visible.height))
                        .filter(|(_, row)| row.commit.is_some())
                        .map(|(index, _)| {
                            (
                                index,
                                Rect::new(
                                    visible.x,
                                    visible.y + (index - first_visible) as u16,
                                    visible.width,
                                    1,
                                ),
                            )
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                layout.graph = Some(SettingsUpdateGraphLayout {
                    rows: visible,
                    scrollbar: track.and_then(|track| project(track).map(|(area, _)| area)),
                    offset,
                    capacity,
                    total,
                    title: project(Rect::new(graph.x, graph.y, graph.width, 1))
                        .map(|(area, _)| area),
                    first_visible,
                    targets,
                });
                let build_clip = Rect::new(build.x, clip.y, build.width, clip.height);
                layout.activity = Some((build_clip, top(build.y), build.height));
                row += height;
                if update.action.is_some() {
                    place_action(&mut layout, row + 1, 1);
                    row += action_height;
                }
            }
        }
        layout.end_row = row;
        layout
    }

    pub(super) fn render(
        &self,
        frame: &mut Frame<'_>,
        detail: Rect,
        update: &SettingsUpdateViewModel,
        model: &SettingsViewModel,
        context: &RenderContext,
    ) {
        let theme = &context.compatibility_theme();
        let focused =
            |field| model.focus == SettingsFocus::Content && model.selected_field == field;
        if let (Some(area), Some(action)) = (self.action, &update.action) {
            let mut button = Button::new("settings.field.startupdate", &action.label)
                .with_bracketed_label(false)
                .with_centered_label(true)
                .with_emphasis(ButtonEmphasis::Primary);
            button.set_focused(focused(SettingsField::StartUpdate));
            button.set_disabled(!action.enabled);
            button.render_inline_frame(frame, area, theme);
        }
        match &update.panel {
            SettingsUpdatePanel::Idle => {}
            SettingsUpdatePanel::Release { notes, activity } => {
                if let Some((y, height)) = self.notes {
                    Surface::new()
                        .titled(i18n::tr!("ui-settings-release-notes"))
                        .render_scrolled_frame(frame, detail, y, height, context);
                    if let Some((area, skipped)) = visible_scrolled_rect(
                        detail.x,
                        y + 1,
                        detail.width,
                        height.saturating_sub(1),
                        detail,
                    ) {
                        frame.render_widget(
                            Paragraph::new(note_lines(notes, detail.width).join("\n"))
                                .scroll((skipped, 0)),
                            area,
                        );
                    }
                }
                if let Some((clip, y, height)) = self.activity {
                    UpdateActivity::new(activity).render_scrolled(frame, clip, y, height, context);
                }
            }
            SettingsUpdatePanel::Source {
                rows,
                message,
                selected,
                selectable,
                activity,
                ..
            } => {
                if let Some((clip, y, height)) = self.activity {
                    UpdateActivity::new(activity).render_scrolled(frame, clip, y, height, context);
                }
                if let Some(graph) = &self.graph {
                    graph.render(
                        frame,
                        rows,
                        message,
                        *selected,
                        *selectable,
                        focused(SettingsField::UpdateGitTree),
                        context,
                    );
                }
            }
        }
    }
}

impl SettingsUpdateGraphLayout {
    fn render(
        &self,
        frame: &mut Frame<'_>,
        rows: &[UpdateGitRow],
        message: &str,
        selected: Option<usize>,
        selectable: bool,
        focused: bool,
        context: &RenderContext,
    ) {
        if let Some(title) = self.title {
            Surface::new()
                .raised(true)
                .titled(i18n::tr!("ui-settings-git-tree"))
                .render_frame(frame, title, context);
        }
        Surface::new().render_frame(frame, self.rows, context);
        if let Some(track) = self.scrollbar {
            Scrollbar::new(self.total, self.capacity, self.offset)
                .render_frame(frame, track, context);
        }
        if rows.is_empty() {
            frame.render_widget(Paragraph::new(message).wrap(Wrap { trim: true }), self.rows);
            return;
        }
        let theme = &context.compatibility_theme();
        let graph_style = Style::default().fg(context.theme.accent);
        for (index, row) in rows
            .iter()
            .enumerate()
            .skip(self.first_visible)
            .take(usize::from(self.rows.height))
        {
            let area = Rect::new(
                self.rows.x,
                self.rows.y + (index - self.first_visible) as u16,
                self.rows.width,
                1,
            );
            let mut spans = vec![Span::styled(
                row.graph
                    .replace('*', crate::cell_icon("clock", "commit"))
                    .replace('|', "│"),
                graph_style,
            )];
            if let Some(commit) = &row.commit {
                let sha = &commit.sha[..8];
                let branches = if commit.branches.is_empty() {
                    String::new()
                } else {
                    format!("({}) ", commit.branches)
                };
                let mut button = Button::new(
                    format!("settings.update.commit.{}", commit.sha),
                    format!("{sha} {branches}{}", commit.subject),
                )
                .with_current(selected == Some(index))
                .with_capture_role("row");
                button.set_focused(focused && selected == Some(index));
                button.set_disabled(!selectable);
                if !selectable && selected == Some(index) {
                    button = button.with_background(context.theme.selected_row_background());
                }
                button.render_surface_frame(frame, area, theme);
                let style = button.surface_style(area, theme);
                spans.push(Span::styled(format!("{sha} "), style));
                spans.push(Span::styled(branches, style.fg(context.theme.accent)));
                spans.push(Span::styled(commit.subject.as_str(), style));
            }
            frame.render_widget(Paragraph::new(Line::from(spans)), area);
        }
    }
}

fn note_lines(notes: &str, width: u16) -> Vec<String> {
    let notes = if notes.trim().is_empty() {
        i18n::tr!("ui-settings-no-release-notes")
    } else {
        notes.to_owned()
    };
    notes
        .lines()
        .flat_map(|line| {
            if line.trim().is_empty() {
                vec![String::new()]
            } else {
                crate::management_wrapped_lines(line, width.max(1))
            }
        })
        .collect()
}
