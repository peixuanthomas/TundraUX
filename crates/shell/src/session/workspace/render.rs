use super::*;

impl Workspace {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::session) fn render_panes(
        &mut self,
        frame: &mut Frame<'_>,
        layout: ui::ShellFrameLayout,
        context: &ui::RenderContext,
        capture: Option<&ui::automation::FrameCapture>,
        controls_blocked: bool,
        picker: Option<&ui::EditorImagePicker>,
        image_caches: &mut std::collections::HashMap<SessionId, PaneImages>,
        image_cleanup: &mut Vec<u8>,
        text_sizing: bool,
    ) {
        let now = Instant::now();
        self.placeholder_redraw_at = None;
        for window in self.windows.values_mut() {
            window.presenting = None;
            window.starting_presenting = false;
        }
        let mut cached_images = HashSet::new();
        let controls_blocked = controls_blocked || self.has_overlay();
        if let Some(capture) = capture {
            capture.set_group("workspace", "toolbar", layout.main, controls_blocked);
        }
        if self.layout.blocked {
            let pane = self.blocked_pane();
            if let Some(pane) = &pane {
                self.render_pane_frame(frame, pane, context, controls_blocked);
            }
            ui::render_workspace_resize_notice(
                frame,
                pane.map_or(layout.main, |pane| pane.content),
                Size::new(self.width, self.height),
                self.layout.size_needed,
                context,
            );
        }
        for pane in &self.layout.panes {
            self.render_pane_frame(frame, pane, context, controls_blocked);
        }
        for pane in &self.layout.panes {
            let Some(window) = self.windows.get_mut(&pane.id) else {
                continue;
            };
            let controls_blocked = controls_blocked || window.input_blocked;
            let focused = Some(pane.id) == self.selection.focused();
            if let Some(capture) = capture {
                let group = format!("session/{}", pane.id);
                capture.set_group(
                    if focused { "page" } else { &group },
                    "page",
                    pane.content,
                    controls_blocked,
                );
            }
            let placeholder = if window.state == ProcessState::Unresponsive
                || window.status.unresponsive
            {
                Some(WorkspacePanePlaceholder::Unresponsive)
            } else if window.launch_error {
                Some(WorkspacePanePlaceholder::Unavailable)
            } else if matches!(window.state, ProcessState::ForceStopping)
                || matches!(
                    window.close,
                    CloseProgress::Cleaning(_) | CloseProgress::Waiting | CloseProgress::Exiting
                )
                || (window.presented.is_none()
                    && matches!(window.close, CloseProgress::Requested(_)))
            {
                Some(WorkspacePanePlaceholder::Closing)
            } else {
                None
            };
            if let Some(placeholder) = placeholder {
                ui::render_workspace_pane_placeholder(frame, pane.content, placeholder, context);
                continue;
            }
            let current = window.frame.as_ref().filter(|rendered| {
                rendered.layout == self.generation
                    && rendered.size == Size::new(pane.content.width, pane.content.height)
            });
            let rendered = current.or(window.presented.as_ref());
            if let Some(rendered) = rendered {
                let current = current.is_some();
                ui::render_workspace_frame(
                    frame,
                    pane.content,
                    rendered,
                    context,
                    !controls_blocked && current,
                );
                if let Some(picker) = picker
                    && !rendered.modal
                    && (!rendered.images.is_empty() || image_caches.contains_key(&pane.id))
                {
                    image_caches.entry(pane.id).or_default().render(
                        frame,
                        pane.content,
                        &rendered.images,
                        picker,
                        context.theme.canvas,
                        self.image_workers.as_ref(),
                    );
                    cached_images.insert(pane.id);
                }
                if text_sizing && !rendered.modal {
                    ui::components::render_workspace_headings(
                        frame,
                        pane.content,
                        &rendered.headings,
                        context,
                    );
                }
                if !controls_blocked
                    && Some(pane.id) == self.input_focus
                    && current
                    && let Some(cursor) = rendered.cursor
                {
                    frame.set_cursor_position((
                        pane.content.x + cursor.x,
                        pane.content.y + cursor.y,
                    ));
                }
                if current {
                    window.presenting = Some(Arc::clone(rendered));
                }
            } else {
                let placeholder = window.initial_placeholder(now);
                ui::render_workspace_pane_placeholder(frame, pane.content, placeholder, context);
                let deadline = if placeholder == WorkspacePanePlaceholder::Starting {
                    window.starting_presenting = true;
                    ui::BusyIndicator::redraw_interval(context.motion, context.capabilities.color)
                        .map(|interval| now + interval)
                } else {
                    window
                        .startup
                        .as_ref()
                        .map(|startup| startup.requested + STARTING_DELAY + Duration::from_nanos(1))
                };
                self.placeholder_redraw_at =
                    self.placeholder_redraw_at.into_iter().chain(deadline).min();
            }
        }
        image_caches.retain(|id, images| {
            if cached_images.contains(id) {
                true
            } else {
                images.cache.clear();
                image_cleanup.extend(images.cache.take_cleanup());
                false
            }
        });
    }

    fn render_pane_frame(
        &self,
        frame: &mut Frame<'_>,
        pane: &WorkspacePaneLayout,
        context: &ui::RenderContext,
        controls_blocked: bool,
    ) {
        let Some(window) = self.windows.get(&pane.id) else {
            return;
        };
        let interaction = WorkspaceWindowButtonsState {
            hovered: self
                .last_pointer
                .filter(|_| self.pointer_hovered && !controls_blocked)
                .and_then(|point| pane.controls.hit_at(point)),
            pressed: self.capture.and_then(|capture| match capture.target {
                PointerTarget::WindowButton(id, action) if id == pane.id => Some(action),
                _ => None,
            }),
            close_disabled: window.close != CloseProgress::Idle,
            ..Default::default()
        };
        ui::render_workspace_pane_frame(
            frame,
            pane,
            &window.launch.title(),
            self.selection.focused() == Some(pane.id),
            interaction,
            context,
        );
    }

    fn taskbar_items(&self, ids: impl Iterator<Item = SessionId>) -> Vec<WorkspaceTaskbarItem> {
        let blocked_window = self.blocked_pane().map(|pane| pane.id);
        ids.filter_map(|id| self.windows.get(&id).map(|window| (id, window)))
            .map(|(id, window)| WorkspaceTaskbarItem {
                id,
                title: window.launch.title(),
                focused: self.selection.focused() == Some(id),
                minimized: blocked_window != Some(id)
                    && !self.layout.panes.iter().any(|pane| pane.id == id),
                closing: window.close != CloseProgress::Idle,
                unresponsive: window.state == ProcessState::Unresponsive
                    || window.status.unresponsive,
            })
            .collect()
    }

    pub(in crate::session) fn render_taskbar(
        &mut self,
        frame: &mut Frame<'_>,
        area: Option<Rect>,
        context: &ui::RenderContext,
        capture: Option<&ui::automation::FrameCapture>,
        controls_blocked: bool,
    ) {
        let Some(area) = area else {
            self.taskbar = None;
            return;
        };
        let items = self.taskbar_items(self.taskbar_order.iter().copied());
        let focused = self.selection.focused();
        let mut taskbar = WorkspaceTaskbar::new(area, &items, self.taskbar_scroll);
        if (self.taskbar_focus != focused
            || self
                .taskbar
                .as_ref()
                .is_none_or(|previous| previous.area != area))
            && let Some(id) = focused
            && !taskbar
                .hits
                .iter()
                .any(|(action, _)| *action == WorkspaceTaskbarAction::Activate(id))
            && let Some(index) = items.iter().position(|item| item.id == id)
        {
            taskbar = WorkspaceTaskbar::new(area, &items, index);
        }
        if self
            .taskbar
            .as_ref()
            .is_some_and(|previous| previous.hits != taskbar.hits)
            && self
                .capture
                .is_some_and(|capture| matches!(capture.target, PointerTarget::Taskbar(_)))
        {
            self.cancel_capture();
        }
        self.taskbar_focus = focused;
        self.taskbar_scroll = taskbar.scroll;
        if let Some(capture) = capture {
            capture.set_group("workspace-taskbar", "toolbar", area, controls_blocked);
        }
        let hovered = self
            .last_pointer
            .filter(|_| self.pointer_hovered && !controls_blocked)
            .and_then(|point| taskbar.hit_at(point));
        let pressed = self.capture.and_then(|capture| match capture.target {
            PointerTarget::Taskbar(action) => Some(action),
            _ => None,
        });
        ui::render_workspace_taskbar(frame, &taskbar, &items, hovered, pressed, context);
        self.taskbar = Some(taskbar);
    }

    pub(in crate::session) fn render_overlay(
        &mut self,
        frame: &mut Frame<'_>,
        context: &ui::RenderContext,
        capture: Option<&ui::automation::FrameCapture>,
        global_modal: bool,
    ) {
        if global_modal {
            return;
        }
        let available = self.layout.available;
        let hovered = self.last_pointer.filter(|_| self.pointer_hovered);
        let pressed = self.capture.and_then(|capture| match capture.target {
            PointerTarget::Overlay(action) => Some(action),
            _ => None,
        });
        if self.chooser.is_some() {
            self.render_chooser(frame, context, capture);
            return;
        }
        if let Some(id) = self.wait_for
            && let Some(window) = self.windows.get(&id)
        {
            if let Some(capture) = capture {
                capture.set_group("workspace-overlay", "dialog", available, false);
            }
            let wait = WorkspaceCloseWait::new(
                available,
                &window.launch.title(),
                id,
                matches!(window.close, CloseProgress::Confirmation { .. }),
            );
            ui::render_workspace_close_wait(
                frame,
                &wait,
                self.modal_focus,
                hovered.and_then(|point| wait.hit_at(point)),
                pressed,
                context,
            );
            self.panel_hits = wait.hits;
        } else if let Some(model) = &mut self.panel {
            if let Some(capture) = capture {
                capture.set_group("workspace", "dialog", available, false);
            }
            model.windows = self
                .windows
                .iter()
                .map(|(&id, window)| WorkspaceWindowViewModel {
                    id,
                    title: window.launch.title(),
                    minimum_size: window.status.minimum,
                    visible: self.layout.panes.iter().any(|pane| pane.id == id),
                    unresponsive: window.state == ProcessState::Unresponsive
                        || window.status.unresponsive,
                    closing: window.close != CloseProgress::Idle,
                })
                .collect();
            model.windows.retain(|window| {
                ui::components::selection_matches(&model.query, [window.title.as_str()])
            });
            if !model
                .windows
                .iter()
                .any(|window| Some(window.id) == model.selected)
            {
                model.selected = model.windows.first().map(|window| window.id);
            }
            model.focused = self.selection.focused();
            model.size_needed = self.layout.size_needed;
            model.capacity = self.layout.capacity;
            model.blocked = self.layout.blocked;
            let panel = WorkspacePanel::new(available, model);
            self.panel_scroll_rows = usize::from(panel.list.height);
            self.panel_max_scroll = panel.max_scroll;
            model.scroll = panel.scroll;
            model.hovered = hovered.and_then(|point| panel.hit_at(point));
            model.pressed = pressed;
            if self.panel_key_focus {
                ui::render_workspace_panel(frame, &panel, model, context);
            } else {
                let mut pointer = model.clone();
                pointer.action_focus = None;
                ui::render_workspace_panel(frame, &panel, &pointer, context);
            }
            self.panel_hits = panel.hits;
        } else {
            self.panel_hits.clear();
            if let Some(menu) = &self.taskbar_menu {
                if let Some(capture) = capture {
                    capture.set_group("workspace-taskbar-menu", "menu", menu.area, false);
                }
                frame.render_widget(ratatui::widgets::Clear, menu.area);
                menu.menu
                    .render_frame(frame, menu.area, &context.compatibility_theme());
            } else {
                self.render_split_menu(frame, context, capture);
            }
        }
    }

    fn render_split_menu(
        &mut self,
        frame: &mut Frame<'_>,
        context: &ui::RenderContext,
        capture: Option<&ui::automation::FrameCapture>,
    ) {
        let Some(id) = self.split_window else {
            return;
        };
        let Some(window) = self.windows.get(&id) else {
            self.close_split_menu();
            return;
        };
        let Some(pane) = self
            .layout
            .panes
            .iter()
            .find(|pane| pane.id == id)
            .copied()
            .or_else(|| self.blocked_pane().filter(|pane| pane.id == id))
        else {
            self.close_split_menu();
            return;
        };
        self.split_model.window = id;
        self.split_model.left_right = self.can_split(id, WorkspaceSplitMode::LeftRight);
        self.split_model.top_bottom = self.can_split(id, WorkspaceSplitMode::TopBottom);
        self.split_model.fullscreen =
            self.selection.visible().len() > 1 && window.close == CloseProgress::Idle;
        let menu = WorkspaceSplitMenu::new(
            self.layout.available,
            pane.controls.split,
            &self.split_model,
        );
        if self
            .split_menu
            .as_ref()
            .is_some_and(|previous| previous.hits != menu.hits)
            && self
                .capture
                .is_some_and(|capture| matches!(capture.target, PointerTarget::Overlay(_)))
        {
            self.cancel_capture();
        }
        self.split_model.hovered = self
            .last_pointer
            .filter(|_| self.pointer_hovered)
            .and_then(|point| menu.hit_at(point));
        self.split_model.pressed = self.capture.and_then(|capture| match capture.target {
            PointerTarget::Overlay(action) => Some(action),
            _ => None,
        });
        if let Some(capture) = capture {
            capture.set_group("workspace-split", "toolbar", menu.area, false);
        }
        if self.panel_key_focus {
            ui::render_workspace_split_menu(frame, &menu, &self.split_model, context);
        } else {
            let mut model = self.split_model.clone();
            model.focused = None;
            ui::render_workspace_split_menu(frame, &menu, &model, context);
        }
        self.split_menu = Some(menu);
    }

    pub(in crate::session) fn has_overlay(&self) -> bool {
        self.panel.is_some()
            || self.wait_for.is_some()
            || self.split_window.is_some()
            || self.taskbar_menu.is_some()
            || self.chooser.is_some()
    }
}
