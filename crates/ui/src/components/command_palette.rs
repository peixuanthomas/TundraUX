use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::{Paragraph, Widget};

use crate::{RenderContext, TundraTheme};

use super::{
    Button, ComponentEvent, ComponentId, ComponentState, DialogLayout, InputEvent, Key, List,
    ListItem, MouseButton, MouseInput, MouseKind, Scrollbar, ScrollbarOrientation, TextInput,
    contains_point,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandPaletteCommand {
    pub id: ComponentId,
    pub title: String,
    pub hint: Option<String>,
    pub group: Option<String>,
    pub keywords: Vec<String>,
    pub disabled: bool,
}

impl CommandPaletteCommand {
    pub fn new(id: impl Into<ComponentId>, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            hint: None,
            group: None,
            keywords: Vec::new(),
            disabled: false,
        }
    }

    pub fn with_group(mut self, group: impl Into<String>) -> Self {
        self.group = Some(group.into());
        self
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    pub fn with_keywords<I, S>(mut self, keywords: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.keywords = keywords.into_iter().map(Into::into).collect();
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandPalette {
    pub id: ComponentId,
    pub commands: Vec<CommandPaletteCommand>,
    pub title: String,
    pub state: ComponentState,
    pub open: bool,
    query_input: TextInput,
    results: List,
    close_button: Button,
    pressed: Option<(ComponentId, Rect)>,
    scroll_offset: Option<usize>,
    scrollbar_grab: Option<u16>,
}

impl CommandPalette {
    pub fn render_with_context(&self, area: Rect, buffer: &mut Buffer, context: &RenderContext) {
        if !self.open {
            return;
        }
        let title = &self.title;
        let _scope = crate::automation::scope(self.id.as_str(), "dialog", &title, area);
        let layout = self.layout(area);
        let theme = DialogLayout::body_theme(context);
        layout.render(buffer, &title, context);
        self.close_button
            .clone()
            .with_background(context.theme.surface)
            .render_borderless(layout.close, buffer, &DialogLayout::close_theme(context));

        let mut input = self.query_input.clone();
        input.set_focused(self.state.focused);
        let prefix = format!("{} ", crate::cell_icon("fileops", "search"));
        input.render_borderless_with_prefix(Self::query_area(layout), buffer, &theme, &prefix);

        let mut results = self
            .results_for_render()
            .with_background(context.theme.surface);
        if let Some((id, _)) = &self.pressed {
            results.set_hovered(results.items.iter().position(|item| &item.id == id));
        }
        let (results_area, track) = self.results_layout(layout, results.items.len());
        if let Some(offset) = self.scroll_offset {
            let max_offset = results
                .items
                .len()
                .saturating_sub(usize::from(results_area.height));
            results.set_viewport_start(offset.min(max_offset));
        }
        if results.items.is_empty() {
            let message = i18n::tr!("management-empty");
            Paragraph::new(message)
                .style(Style::default().fg(context.theme.muted))
                .render(
                    Rect::new(
                        layout.body.x,
                        results_area.y,
                        layout.body.width,
                        results_area.height,
                    ),
                    buffer,
                );
        } else {
            results.render_borderless(results_area, buffer, &theme);
        }
        if let Some(track) = track {
            Scrollbar::new(
                results.items.len(),
                usize::from(track.height),
                self.scroll_offset.unwrap_or(0),
            )
            .render(track, buffer, context);
        }
    }

    pub fn new(id: impl Into<ComponentId>, commands: Vec<CommandPaletteCommand>) -> Self {
        let id = id.into();
        let mut query_input = TextInput::new(format!("{}.query", id.as_str()))
            .with_placeholder(i18n::tr!("ui-components-type-a-command"));
        query_input.set_focused(false);
        let mut results = Self::make_results(&id, &commands, "");
        results.set_focused(false);
        Self {
            close_button: Button::new(format!("{}.close", id.as_str()), Key::Escape.label())
                .with_bracketed_label(false),
            id,
            commands,
            title: i18n::tr!("ui-components-command-palette"),
            state: ComponentState::default(),
            open: false,
            query_input,
            results,
            pressed: None,
            scroll_offset: None,
            scrollbar_grab: None,
        }
    }

    pub fn titled(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    pub fn with_placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.query_input = self.query_input.with_placeholder(placeholder);
        self
    }

    pub fn with_scrollbar(mut self) -> Self {
        self.scroll_offset = Some(0);
        self
    }

    pub fn input_area(&self, area: Rect) -> Rect {
        Self::query_edit_area(self.layout(area))
    }

    pub fn cancel_pointer(&mut self) {
        self.pressed = None;
        self.scrollbar_grab = None;
        self.results.set_hovered(None);
    }

    pub fn open(&mut self) {
        self.set_open(true);
    }

    pub fn close(&mut self) {
        self.set_open(false);
    }

    pub fn set_open(&mut self, open: bool) {
        self.open = open;
        self.state.focused = open;
        self.query_input.set_focused(open);
        self.results.set_focused(open);
        if !open {
            self.scrollbar_grab = None;
            self.results.set_hovered(None);
            self.close_button.set_focused(false);
            self.close_button.state.hovered = false;
            self.pressed = None;
        } else {
            self.sync_results(false);
        }
    }

    pub fn query(&self) -> &str {
        self.query_input.value()
    }

    pub fn set_query(&mut self, query: impl Into<String>) {
        self.query_input.set_value(query);
        self.sync_results(true);
    }

    pub fn set_selected(&mut self, index: Option<usize>) {
        self.sync_results(false);
        self.results.set_selected(index);
    }

    pub fn selected_command(&self) -> Option<&CommandPaletteCommand> {
        let selected = self.results.selected_item()?;
        self.commands
            .iter()
            .find(|command| command.id == selected.id)
    }

    pub fn visible_commands(&self) -> Vec<&CommandPaletteCommand> {
        self.visible_indices()
            .into_iter()
            .filter_map(|index| self.commands.get(index))
            .collect()
    }

    pub fn handle_event(&mut self, event: InputEvent, area: Rect) -> ComponentEvent {
        if !self.open {
            return ComponentEvent::None;
        }

        self.sync_results(false);
        let layout = self.layout(area);
        let (results_area, track) = self.results_layout(layout, self.results.items.len());
        let rows = usize::from(results_area.height);
        let max_offset = self.results.items.len().saturating_sub(rows);
        if let Some(offset) = &mut self.scroll_offset {
            *offset = (*offset).min(max_offset);
            self.results.set_viewport_start(*offset);
        }
        if !matches!(event, InputEvent::Mouse(_)) {
            self.pressed = None;
            self.scrollbar_grab = None;
            self.close_button.set_focused(false);
        }
        match event {
            InputEvent::Key(key) if !key.is_press_like() => ComponentEvent::None,
            InputEvent::FocusGained => {
                self.state.focused = true;
                self.query_input.set_focused(true);
                self.results.set_focused(true);
                ComponentEvent::Consumed
            }
            InputEvent::FocusLost => {
                self.close();
                ComponentEvent::Dismissed(self.id.clone())
            }
            InputEvent::Key(key) if key.key == Key::Escape => {
                self.close();
                ComponentEvent::Dismissed(self.id.clone())
            }
            InputEvent::Key(key) if key.key == Key::Enter => {
                if let (Some(offset), Some(index)) =
                    (&mut self.scroll_offset, self.results.selected_index())
                    && !(*offset..offset.saturating_add(rows)).contains(&index)
                {
                    *offset = (*offset).clamp(List::automatic_viewport_start(index, rows), index);
                    return ComponentEvent::Changed(self.id.clone());
                }
                self.activate_selected()
            }
            InputEvent::Key(key)
                if matches!(key.key, Key::Up | Key::Down | Key::Tab | Key::BackTab)
                    || key.modifiers.is_control_only()
                        && matches!(key.key, Key::Home | Key::End) =>
            {
                let list_key = match key.key {
                    Key::Tab => Key::Down,
                    Key::BackTab => Key::Up,
                    other => other,
                };
                let event = self.results.handle_event_borderless(
                    InputEvent::key_with_modifiers(list_key, key.modifiers),
                    results_area,
                );
                if let (Some(offset), Some(index)) =
                    (&mut self.scroll_offset, self.results.selected_index())
                {
                    *offset = (*offset).clamp(List::automatic_viewport_start(index, rows), index);
                }
                self.translate_results_event(event)
            }
            event @ (InputEvent::Key(_) | InputEvent::Paste(_)) => {
                let event = self
                    .query_input
                    .handle_event_borderless(event, Self::query_edit_area(layout));
                if matches!(event, ComponentEvent::Changed(_)) {
                    self.sync_results(true);
                    ComponentEvent::Changed(self.id.clone())
                } else {
                    ComponentEvent::Consumed
                }
            }
            InputEvent::Mouse(mouse) => {
                let point = (mouse.column(), mouse.row());
                if let Some(track) = track
                    && self.handle_scrollbar(mouse, Self::results_area(layout), track, max_offset)
                {
                    return ComponentEvent::Changed(self.id.clone());
                }
                if self.scroll_offset.is_some()
                    && matches!(mouse.kind, MouseKind::ScrollUp | MouseKind::ScrollDown)
                {
                    self.cancel_pointer();
                    return ComponentEvent::Consumed;
                }
                let inside = contains_point(area, point.0, point.1);
                if !inside
                    && matches!(
                        mouse.kind,
                        MouseKind::Down(_) | MouseKind::Click(_) | MouseKind::DoubleClick(_)
                    )
                {
                    self.close();
                    return ComponentEvent::Dismissed(self.id.clone());
                }

                if matches!(mouse.kind, MouseKind::Down(_)) {
                    self.close_button.set_focused(false);
                } else if mouse.kind == MouseKind::Move
                    && !contains_point(layout.close, point.0, point.1)
                {
                    self.close_button.state.active = false;
                }
                if matches!(
                    self.close_button
                        .handle_event(InputEvent::Mouse(mouse), layout.close),
                    ComponentEvent::Activated(_)
                ) {
                    self.close();
                    return ComponentEvent::Dismissed(self.id.clone());
                }
                let row = self
                    .results
                    .borderless_item_index_at(results_area, point.0, point.1)
                    .and_then(|index| self.results.items.get(index))
                    .filter(|item| !item.disabled)
                    .map(|item| {
                        (
                            item.id.clone(),
                            Rect::new(results_area.x, point.1, results_area.width, 1),
                        )
                    });
                match mouse.kind {
                    MouseKind::Down(MouseButton::Left) => {
                        self.pressed = row;
                        if self.pressed.is_some() {
                            return ComponentEvent::Changed(self.id.clone());
                        }
                    }
                    MouseKind::Drag(MouseButton::Left) | MouseKind::Move => {
                        if self.pressed != row {
                            self.pressed = None;
                        }
                    }
                    MouseKind::Up(MouseButton::Left) => {
                        if let Some(pressed) = self.pressed.take()
                            && row.as_ref() == Some(&pressed)
                        {
                            self.close();
                            return ComponentEvent::Activated(pressed.0);
                        }
                    }
                    _ => self.pressed = None,
                }
                let input_event = self.query_input.handle_event_borderless(
                    InputEvent::Mouse(mouse),
                    Self::query_edit_area(layout),
                );
                let results_event = self
                    .results
                    .handle_event_borderless(InputEvent::Mouse(mouse), results_area);
                if mouse.kind == MouseKind::Click(MouseButton::Left)
                    && matches!(results_event, ComponentEvent::Selected(_, _))
                {
                    return self.activate_selected();
                }
                if let ComponentEvent::Activated(id) = results_event {
                    self.close();
                    return ComponentEvent::Activated(id);
                }
                if matches!(input_event, ComponentEvent::Changed(_)) {
                    self.sync_results(true);
                    return ComponentEvent::Changed(self.id.clone());
                }
                if matches!(
                    input_event,
                    ComponentEvent::Changed(_) | ComponentEvent::FocusRequested(_)
                ) || matches!(results_event, ComponentEvent::Changed(_))
                {
                    ComponentEvent::Changed(self.id.clone())
                } else {
                    self.translate_results_event(results_event)
                }
            }
            _ => ComponentEvent::Consumed,
        }
    }

    pub fn render(&self, area: Rect, buffer: &mut Buffer, theme: &TundraTheme) {
        self.render_with_context(area, buffer, &DialogLayout::surface_theme(theme));
    }

    /// Renders the command palette through a Ratatui [`Frame`].
    pub fn render_frame(&self, frame: &mut Frame<'_>, area: Rect, theme: &TundraTheme) {
        self.render(area, frame.buffer_mut(), theme);
    }

    fn visible_indices(&self) -> Vec<usize> {
        Self::visible_indices_for(self.query_input.value(), &self.commands)
    }

    fn visible_indices_for(query: &str, commands: &[CommandPaletteCommand]) -> Vec<usize> {
        commands
            .iter()
            .enumerate()
            .filter_map(|(index, command)| {
                let values = std::iter::once(command.title.as_str())
                    .chain(command.hint.iter().map(String::as_str))
                    .chain(command.keywords.iter().map(String::as_str));
                super::selection_matches(query, values).then_some(index)
            })
            .collect()
    }

    fn activate_selected(&mut self) -> ComponentEvent {
        let Some(command) = self.selected_command() else {
            return ComponentEvent::Consumed;
        };
        if command.disabled {
            return ComponentEvent::Consumed;
        }

        let id = command.id.clone();
        self.close();
        ComponentEvent::Activated(id)
    }

    fn translate_results_event(&mut self, event: ComponentEvent) -> ComponentEvent {
        match event {
            ComponentEvent::Selected(_, index) => ComponentEvent::Selected(self.id.clone(), index),
            ComponentEvent::Changed(_) => ComponentEvent::Changed(self.id.clone()),
            ComponentEvent::Activated(id) => {
                self.close();
                ComponentEvent::Activated(id)
            }
            ComponentEvent::None | ComponentEvent::FocusRequested(_) => ComponentEvent::Consumed,
            other => other,
        }
    }

    fn sync_results(&mut self, reset_selection: bool) {
        if reset_selection && let Some(offset) = &mut self.scroll_offset {
            *offset = 0;
        }
        let selected_id = (!reset_selection)
            .then(|| self.results.selected_item().map(|item| item.id.clone()))
            .flatten();
        let hovered = self.results.hovered_index();
        let mut results = Self::make_results(&self.id, &self.commands, self.query_input.value());
        if let Some(selected_id) = selected_id {
            let selected = results
                .items
                .iter()
                .position(|item| item.id == selected_id && !item.disabled);
            results.set_selected(selected);
        }
        if !reset_selection {
            results.set_hovered(hovered);
        }
        results.state = self.state;
        results.set_focused(self.open && self.state.focused);
        self.results = results;
    }

    fn results_for_render(&self) -> List {
        let mut results = Self::make_results(&self.id, &self.commands, self.query_input.value());
        if let Some(selected) = self.results.selected_item() {
            let selected_index = results
                .items
                .iter()
                .position(|item| item.id == selected.id && !item.disabled);
            results.set_selected(selected_index);
        }
        results.set_hovered(self.results.hovered_index());
        results.state = self.state;
        results
    }

    fn make_results(id: &ComponentId, commands: &[CommandPaletteCommand], query: &str) -> List {
        let mut previous_group = None;
        let mut items = Vec::new();
        for index in Self::visible_indices_for(query, commands) {
            let command = &commands[index];
            if command.group != previous_group {
                if let Some(group) = &command.group {
                    items.push(
                        ListItem::new(format!("{}.group.{}", id.as_str(), index), group)
                            .disabled(true),
                    );
                }
                previous_group = command.group.clone();
            }
            let mut item =
                ListItem::new(command.id.clone(), command.title.clone()).disabled(command.disabled);
            if let Some(hint) = &command.hint {
                item = item.with_description(hint);
            }
            items.push(item);
        }
        List::new(format!("{}.results", id.as_str()), items)
            .with_highlight_symbol(Some(format!("{}   ", crate::cell_icon("nav", "current"))))
    }

    fn layout(&self, area: Rect) -> DialogLayout {
        DialogLayout::with_min_body(area, 0, Some(&self.close_button.label), 2)
    }

    fn query_area(layout: DialogLayout) -> Rect {
        let body = layout.body;
        Rect::new(body.x, body.y, body.width, body.height.min(1))
    }

    fn query_edit_area(layout: DialogLayout) -> Rect {
        let query = Self::query_area(layout);
        Rect::new(
            query.x.saturating_add(2),
            query.y,
            query.width.saturating_sub(2),
            query.height,
        )
    }

    fn results_area(layout: DialogLayout) -> Rect {
        let body = layout.body;
        let offset = body.height.min(1) + u16::from(body.height >= 3);
        Rect::new(
            layout.area.x,
            body.y + offset,
            layout.area.width,
            body.height - offset,
        )
    }

    fn results_layout(&self, layout: DialogLayout, content_len: usize) -> (Rect, Option<Rect>) {
        let mut rows = Self::results_area(layout);
        if self.scroll_offset.is_none()
            || rows.width < 3
            || rows.height == 0
            || content_len <= usize::from(rows.height)
        {
            return (rows, None);
        }
        let track = Rect::new(rows.right() - 1, rows.y, 1, rows.height);
        rows.width -= 2;
        (rows, Some(track))
    }

    fn handle_scrollbar(
        &mut self,
        mouse: MouseInput,
        area: Rect,
        track: Rect,
        max_offset: usize,
    ) -> bool {
        let offset = self.scroll_offset.expect("scrollable results");
        let point = (mouse.column(), mouse.row());
        let (start, len) =
            Scrollbar::new(self.results.items.len(), usize::from(track.height), offset)
                .thumb_range(track);
        let row = point.1.saturating_sub(track.y);
        let next = match mouse.kind {
            MouseKind::ScrollUp | MouseKind::ScrollDown if area.contains(point.into()) => {
                self.scrollbar_grab = None;
                offset.saturating_add_signed(if mouse.kind == MouseKind::ScrollDown {
                    1
                } else {
                    -1
                })
            }
            MouseKind::Down(MouseButton::Left) | MouseKind::Click(MouseButton::Left)
                if area.contains(point.into())
                    && ScrollbarOrientation::VerticalRight.hit_test(track, point) =>
            {
                if (start..start + len).contains(&row) {
                    self.scrollbar_grab = Some(row - start);
                    offset
                } else {
                    self.scrollbar_grab = Some(len / 2);
                    scrollbar_offset(row.saturating_sub(len / 2), track.height - len, max_offset)
                }
            }
            MouseKind::Drag(MouseButton::Left) => {
                let Some(grab) = self.scrollbar_grab else {
                    return false;
                };
                scrollbar_offset(row.saturating_sub(grab), track.height - len, max_offset)
            }
            MouseKind::Up(MouseButton::Left) if self.scrollbar_grab.take().is_some() => offset,
            _ => return false,
        };
        self.pressed = None;
        self.results.set_hovered(None);
        self.scroll_offset = Some(next.min(max_offset));
        true
    }
}

fn scrollbar_offset(row: u16, travel: u16, max_offset: usize) -> usize {
    let travel = usize::from(travel.max(1));
    usize::from(row)
        .min(travel)
        .saturating_mul(max_offset)
        .div_ceil(travel)
}
