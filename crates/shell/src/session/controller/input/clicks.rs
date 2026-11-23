use crate::session::*;

impl TimedClick {
    fn near(&self, point: CellPosition) -> bool {
        self.coordinates.0.abs_diff(point.0) <= 1 && self.coordinates.1.abs_diff(point.1) <= 1
    }
}

impl ShellSession {
    fn click_item_at(&self, target: Option<ShellComponent>, point: CellPosition) -> ClickItem {
        match target {
            Some(ShellComponent::Explorer) => {
                if let Some(ui::ExplorerHitTarget::Entry(index)) =
                    self.explorer_hit_target_at(point)
                    && let Some(entry) =
                        self.app.explorer_state().and_then(|s| s.entries.get(index))
                {
                    return ClickItem::File(entry.path.clone());
                }
            }
            Some(ShellComponent::Home) => {
                if let Some(index) = self.home_entry_index_at(point)
                    && let Some(entry) = self.user_home_entries().get(index)
                {
                    return ClickItem::Named(entry.icon_identity().to_owned());
                }
            }
            Some(ShellComponent::Launcher) => {
                let model = self.to_launcher_view_model();
                let layout = ui::launcher_layout(self.touch_main(), &model);
                if let Some(ui::LauncherHitTarget::Item(index)) = layout.hit_test(point.0, point.1)
                {
                    return ClickItem::Named(model.items[index].id.clone());
                }
            }
            Some(ShellComponent::Logs) => {
                let model = self.to_logs_view_model();
                if let Some(ui::LogsHitTarget::Event(index)) =
                    ui::logs_hit_test(self.touch_main(), &model, point)
                    && let Some(event) = model.events.get(index)
                {
                    return ClickItem::Named(event.id.clone());
                }
            }
            Some(ShellComponent::SystemStatus) => {
                if let Some((_, layout)) = self.system_status_layout()
                    && let Some(ui::SystemStatusHitTarget::Widget(kind)) =
                        ui::system_status_hit_test(&layout, point)
                {
                    return ClickItem::Widget(kind);
                }
            }
            _ => {}
        }
        self.button_regions
            .iter()
            .rev()
            .find(|button| !button.disabled && rect_contains(button.area, point))
            .map_or(ClickItem::Cell(point), |button| {
                ClickItem::Named(button.id.as_str().to_owned())
            })
    }

    pub(in crate::session) fn update_click_record(&mut self, input: &InputEvent, at: Instant) {
        let Some(last) = &self.last_click else { return };
        match input {
            InputEvent::Mouse(mouse) => match mouse.kind {
                ui::MouseEventKind::Moved | ui::MouseEventKind::Up(PointerButton::Left) => {
                    let point = mouse.coordinates();
                    if last.coordinates != point
                        && (!last.near(point)
                            || self.hit_map.target_at(point) != last.target
                            || self.click_item_at(last.target, point) != last.item)
                    {
                        self.last_click = None;
                    } else if matches!(mouse.kind, ui::MouseEventKind::Up(_))
                        && let Some(last) = self.last_click.as_mut()
                        && !last.released
                    {
                        last.at = at;
                        last.released = true;
                    }
                }
                ui::MouseEventKind::Scroll(_)
                | ui::MouseEventKind::Drag(_)
                | ui::MouseEventKind::Down(PointerButton::Right | PointerButton::Middle) => {
                    self.last_click = None
                }
                _ => {}
            },
            InputEvent::Key(_)
            | InputEvent::Paste(_)
            | InputEvent::Resize { .. }
            | InputEvent::FocusLost => self.last_click = None,
            _ => {}
        }
    }

    pub(in crate::session) fn register_click(
        &mut self,
        target: Option<ShellComponent>,
        coordinates: CellPosition,
        button: PointerButton,
        received_at: Instant,
    ) -> ClickKind {
        if button != PointerButton::Left {
            self.last_click = None;
            return ClickKind::Single;
        }
        let page = self.active_page_id();
        let item = self.click_item_at(target, coordinates);
        if self.last_click.as_ref().is_some_and(|last| {
            last.released
                && last.page == page
                && last.target == target
                && last.item == item
                && last.near(coordinates)
                && received_at
                    .checked_duration_since(last.at)
                    .is_some_and(|elapsed| elapsed <= DOUBLE_CLICK_INTERVAL)
        }) {
            self.last_click = None;
            return ClickKind::Double;
        }
        self.last_click = Some(TimedClick {
            page,
            target,
            item,
            coordinates,
            at: received_at,
            released: self.interaction.completed_button_click_at.take() == Some(received_at),
        });
        ClickKind::Single
    }
}
