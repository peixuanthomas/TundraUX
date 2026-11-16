use super::{WorkspaceAction, WorkspaceSplitMode};
use crate::components::{Button, ComponentState, terminal_width};
use crate::{RenderContext, automation};
use ratatui::{Frame, layout::Rect, style::Style, widgets::Clear};
use workspace_protocol::SessionId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceSplitMenuViewModel {
    pub window: SessionId,
    pub left_right: bool,
    pub top_bottom: bool,
    pub fullscreen: bool,
    pub focused: Option<WorkspaceAction>,
    pub hovered: Option<WorkspaceAction>,
    pub pressed: Option<WorkspaceAction>,
}

impl Default for WorkspaceSplitMenuViewModel {
    fn default() -> Self {
        Self {
            window: SessionId(0),
            left_right: false,
            top_bottom: false,
            fullscreen: false,
            focused: None,
            hovered: None,
            pressed: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceSplitMenu {
    pub area: Rect,
    pub hits: Vec<(WorkspaceAction, Rect)>,
    items: Vec<(WorkspaceAction, String, bool, Rect)>,
}

impl WorkspaceSplitMenu {
    pub fn new(available: Rect, anchor: Rect, model: &WorkspaceSplitMenuViewModel) -> Self {
        let options = [
            (
                WorkspaceAction::SetMode(WorkspaceSplitMode::LeftRight),
                i18n::tr!("ui-workspace-left-right"),
                model.left_right,
            ),
            (
                WorkspaceAction::SetMode(WorkspaceSplitMode::TopBottom),
                i18n::tr!("ui-workspace-top-bottom"),
                model.top_bottom,
            ),
            (
                WorkspaceAction::Maximize(model.window),
                i18n::tr!("ui-workspace-maximize"),
                model.fullscreen,
            ),
            (
                WorkspaceAction::Dismiss,
                i18n::tr!("ui-workspace-dismiss"),
                true,
            ),
        ];
        let width = (options
            .iter()
            .map(|(_, label, _)| terminal_width(label))
            .max()
            .unwrap_or(0)
            + 4)
        .max(18)
        .min(usize::from(available.width)) as u16;
        let height = 4.min(available.height);
        let x = anchor.x.min(available.right() - width).max(available.x);
        let y = anchor
            .bottom()
            .min(available.bottom() - height)
            .max(available.y);
        let area = Rect::new(x, y, width, height);
        let items = options
            .into_iter()
            .take(usize::from(height))
            .enumerate()
            .map(|(index, (action, label, enabled))| {
                (
                    action,
                    label,
                    enabled,
                    Rect::new(x, y + index as u16, width, 1),
                )
            })
            .collect::<Vec<_>>();
        let hits = items
            .iter()
            .filter(|(_, _, enabled, _)| *enabled)
            .map(|(action, _, _, area)| (*action, *area))
            .collect();
        Self { area, hits, items }
    }

    pub fn hit_at(&self, point: (u16, u16)) -> Option<WorkspaceAction> {
        self.hits
            .iter()
            .find(|(_, area)| area.contains(point.into()))
            .map(|(action, _)| *action)
    }
}

pub fn render_workspace_split_menu(
    frame: &mut Frame<'_>,
    menu: &WorkspaceSplitMenu,
    model: &WorkspaceSplitMenuViewModel,
    context: &RenderContext,
) {
    let _scope = automation::scope(
        "split-menu",
        "menu",
        &i18n::tr!("ui-workspace-split"),
        menu.area,
    );
    frame.render_widget(Clear, menu.area);
    frame
        .buffer_mut()
        .set_style(menu.area, Style::default().bg(context.theme.raised));
    let mut theme = context.compatibility_theme();
    theme.buttons = None;
    for (action, label, enabled, area) in &menu.items {
        let mut button = Button::new(action.id(), label)
            .with_centered_label(true)
            .with_background(context.theme.raised);
        button.state = ComponentState::default()
            .disabled(!enabled)
            .focused(model.focused == Some(*action))
            .hovered(model.hovered == Some(*action))
            .active(model.pressed == Some(*action));
        button.render_borderless_frame(frame, *area, &theme);
    }
}
