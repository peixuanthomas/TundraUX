use crate::session::*;

#[cfg(test)]
#[path = "../../../../tests/unit/session/controller/home_navigation/touch_tests.rs"]
mod touch_tests;

impl ShellSession {
    pub(in crate::session) fn user_home_entries(&self) -> Vec<ui::ShellEntry> {
        let _language = i18n::enter_snapshot(self.language.clone());
        if self.is_strict_guest() {
            return Vec::new();
        }
        let mut entries = user_home_entries();
        let applications = self.available_built_in_applications();
        let tasks = [
            "packages",
            "services",
            "processes",
            "network",
            "disks",
            "firewall",
            "ssh",
            "users",
            "scheduled-tasks",
            "tasks",
            "logs",
            "backups",
        ]
        .into_iter()
        .filter_map(|key| {
            let application = applications
                .iter()
                .find(|application| application.id.strip_prefix("builtin.") == Some(key))?;
            Some(
                ui::ShellEntry::new(
                    application.localized_name().render_current(),
                    application.localized_description().render_current(),
                )
                .with_icon_key(key),
            )
        });
        entries.splice(1..1, tasks);
        entries.sort_by_key(|entry| {
            self.pages
                .home
                .order
                .iter()
                .position(|id| id == entry.icon_identity())
                .unwrap_or(usize::MAX)
        });
        entries
    }

    pub(in crate::session) fn is_strict_guest(&self) -> bool {
        self.app
            .auth_session()
            .is_some_and(|session| session.role == UserRole::Guest)
    }

    pub(in crate::session) fn sync_home_entry_selection(&mut self) {
        let count = self.user_home_entries().len();
        self.pages.home.selected_home_entry_index = if count == 0 {
            0
        } else {
            self.pages.home.selected_home_entry_index.min(count - 1)
        };
        self.sync_home_viewport();
    }

    pub(in crate::session) fn select_home_entry(&mut self, index: usize) {
        let entries = self.user_home_entries();
        if entries.is_empty() {
            self.pages.home.selected_home_entry_index = 0;
            return;
        }

        self.pages.home.selected_home_entry_index = index.min(entries.len() - 1);
        self.sync_home_viewport();
        self.notify_status(i18n::LocalizedText::from(i18n::msg!(
            "shell-home-arg1",
            arg1 = entries[self.pages.home.selected_home_entry_index]
                .label
                .clone()
        )));
    }

    pub(in crate::session) fn select_home_entry_delta(&mut self, delta: isize) {
        let count = self.user_home_entries().len();
        if count == 0 {
            self.pages.home.selected_home_entry_index = 0;
            return;
        }

        let current = self.selected_home_entry_index().min(count - 1) as isize;
        let next = (current + delta).clamp(0, count.saturating_sub(1) as isize);
        self.select_home_entry(next as usize);
    }

    pub(in crate::session) fn select_home_entry_row_delta(&mut self, direction: isize) {
        let columns = self.visible_home_entry_columns().max(1) as isize;
        self.select_home_entry_delta(direction.saturating_mul(columns));
    }

    pub(in crate::session) fn activate_selected_home_entry(&mut self, platform: &dyn Platform) {
        self.activate_home_entry(self.selected_home_entry_index(), platform);
    }

    pub(in crate::session) fn activate_home_entry(
        &mut self,
        index: usize,
        platform: &dyn Platform,
    ) {
        if self.pages.home.editing {
            return;
        }
        let entries = self.user_home_entries();
        let Some(entry) = entries.get(index) else {
            return;
        };

        self.pages.home.selected_home_entry_index = index;
        match entry.icon_identity() {
            "explorer" => self.open_explorer(platform),
            "launcher" => self.open_launcher(platform),
            "settings" => self.open_settings(),
            "system_status" => self.open_system_status(),
            "users" => self.open_user_management(),
            "tasks" => self.toggle_tasks(),
            "logs" => self.open_logs(),
            "packages" => self.open_management(platform::management::ManagementKind::Packages),
            "services" => self.open_management(platform::management::ManagementKind::Services),
            "processes" => self.open_management(platform::management::ManagementKind::Processes),
            "network" => self.open_management(platform::management::ManagementKind::Network),
            "disks" => self.open_management(platform::management::ManagementKind::Disks),
            "firewall" => self.open_management(platform::management::ManagementKind::Firewall),
            "ssh" => self.open_management(platform::management::ManagementKind::Ssh),
            "backups" => self.open_management(platform::management::ManagementKind::Backups),
            "scheduled-tasks" => {
                self.open_management(platform::management::ManagementKind::ScheduledTasks)
            }
            _ => {}
        }
    }

    pub(in crate::session) fn visible_home_entry_columns(&self) -> usize {
        self.home_content_layout()
            .map_or(1, |layout| layout.columns)
    }

    pub(in crate::session) fn home_entry_index_at(
        &self,
        coordinates: CellPosition,
    ) -> Option<usize> {
        self.home_content_layout()?.entry_at(coordinates)
    }

    pub(in crate::session) fn home_content_layout(&self) -> Option<ui::HomeLayout> {
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let ui::ShellLayout::Full { main, .. } = self.shell_layout_for(area) else {
            return None;
        };
        Some(ui::home_layout(main, &self.to_home_view_model()))
    }

    fn sync_home_viewport(&mut self) {
        if let Some(layout) = self.home_content_layout() {
            self.pages.home.home_viewport_offset = layout.visible_start;
        }
    }

    pub(in crate::session) fn handle_home_pointer_scrollbar(&mut self, mouse: &MouseInput) -> bool {
        if self.active_screen() != ShellScreen::Home {
            if matches!(self.scrollbar_drag, Some(ScrollbarDragState::Home { .. })) {
                self.scrollbar_drag = None;
            }
            return false;
        }
        match mouse.kind {
            ui::MouseEventKind::Down(PointerButton::Left) => {
                let Some(layout) = self.home_content_layout() else {
                    return false;
                };
                let Some(track) = layout.scrollbar else {
                    return false;
                };
                if !ScrollbarOrientation::VerticalRight.hit_test(track, mouse.coordinates()) {
                    return false;
                }
                let (start, height) = ui::components::Scrollbar::new(
                    layout.scroll_content_len,
                    layout.visible_capacity,
                    layout.visible_start,
                )
                .thumb_range(track);
                let thumb = Rect::new(track.x, track.y.saturating_add(start), track.width, height);
                let grab_offset =
                    if ScrollbarOrientation::VerticalRight.hit_test(thumb, mouse.coordinates()) {
                        mouse.coordinates().1.saturating_sub(thumb.y)
                    } else {
                        height / 2
                    };
                self.scrollbar_drag = Some(ScrollbarDragState::Home { grab_offset });
                self.interaction.button_capture = None;
                self.drag_tracker = None;
                self.drag_home_scrollbar(mouse.coordinates(), grab_offset);
                true
            }
            ui::MouseEventKind::Drag(PointerButton::Left)
            | ui::MouseEventKind::Up(PointerButton::Left) => {
                let Some(ScrollbarDragState::Home { grab_offset }) = self.scrollbar_drag else {
                    return false;
                };
                self.drag_home_scrollbar(mouse.coordinates(), grab_offset);
                if matches!(mouse.kind, ui::MouseEventKind::Up(_)) {
                    self.scrollbar_drag = None;
                }
                true
            }
            _ => false,
        }
    }

    fn drag_home_scrollbar(&mut self, coordinates: CellPosition, grab_offset: u16) {
        let Some(layout) = self.home_content_layout() else {
            return;
        };
        let Some(track) = layout.scrollbar else {
            self.scrollbar_drag = None;
            return;
        };
        let (_, height) = ui::components::Scrollbar::new(
            layout.scroll_content_len,
            layout.visible_capacity,
            layout.visible_start,
        )
        .thumb_range(track);
        let requested = scrollbar_window_start(
            coordinates.1,
            grab_offset,
            track.y,
            track.height,
            height,
            layout.scroll_content_len,
            layout.visible_capacity,
        );
        self.pages.home.home_viewport_offset = requested / layout.columns * layout.columns;
        let count = self.user_home_entries().len();
        if count > 0 {
            let last = self
                .pages
                .home
                .home_viewport_offset
                .saturating_add(layout.visible_capacity)
                .saturating_sub(1)
                .min(count - 1);
            self.pages.home.selected_home_entry_index = self
                .pages
                .home
                .selected_home_entry_index
                .clamp(self.pages.home.home_viewport_offset.min(last), last);
        }
    }

    pub(in crate::session) fn notification_action_index_at(
        &self,
        coordinates: CellPosition,
    ) -> Option<usize> {
        let model = self.notification_active_modal_view_model()?;
        let area = self.shell_modal_area();
        let ui::NotificationLayout::Dialog(layout) = ui::notification_layout(area, &model) else {
            return None;
        };

        layout
            .actions
            .iter()
            .find(|action| rect_contains(action.area, coordinates))
            .map(|action| action.index)
    }

    pub(in crate::session) fn notification_can_render(&self) -> bool {
        let Some(model) = self.notification_active_modal_view_model() else {
            return false;
        };
        let area = self.shell_modal_area();
        matches!(
            ui::notification_layout(area, &model),
            ui::NotificationLayout::Dialog(_)
        )
    }
}
