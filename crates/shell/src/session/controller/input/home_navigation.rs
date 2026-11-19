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
        let mut entries = self.default_home_entries();
        if let Some(shortcuts) = &self.pages.home.shortcuts {
            entries.retain(|entry| {
                entry.icon_identity() == "launcher"
                    || shortcuts.iter().any(|key| key == entry.icon_identity())
            });
            for application in self.available_built_in_applications() {
                let key = Self::home_shortcut_key(application.id);
                if shortcuts.contains(&key)
                    && !entries.iter().any(|entry| entry.icon_identity() == key)
                {
                    entries.push(
                        ui::ShellEntry::new(
                            application.localized_name().render_current(),
                            application.localized_description().render_current(),
                        )
                        .with_icon_key(key),
                    );
                }
            }
            for record in &self.app.storage_config().launcher.entries {
                let key = Self::home_shortcut_key(&record.id);
                if shortcuts.contains(&key) {
                    entries.push(
                        ui::ShellEntry::new(
                            app::launcher::launcher_entry_name(record),
                            &record.path,
                        )
                        .with_icon_key(key),
                    );
                }
            }
        }
        entries.sort_by_key(|entry| {
            let key = entry.icon_identity();
            (
                self.pages
                    .home
                    .order
                    .iter()
                    .position(|id| id == key)
                    .unwrap_or(usize::MAX),
                self.pages
                    .home
                    .shortcuts
                    .as_ref()
                    .and_then(|shortcuts| shortcuts.iter().position(|id| id == key))
                    .unwrap_or(usize::MAX),
            )
        });
        entries
    }

    pub(in crate::session) fn home_shortcut_key(id: &str) -> String {
        match id.strip_prefix("builtin.") {
            Some("system-status") => "system_status".into(),
            Some(key) => key.into(),
            None => format!("launcher.{id}"),
        }
    }

    pub(in crate::session) fn default_home_entries(&self) -> Vec<ui::ShellEntry> {
        let _language = i18n::enter_snapshot(self.language.clone());
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
            "launcher" => self.open_launcher(platform),
            "users" => self.open_user_management(),
            "tasks" => self.toggle_tasks(),
            key => {
                if let Some(launch) = WindowLaunch::for_builtin(key) {
                    launch.activate(self, platform);
                } else if let Some(id) = key.strip_prefix("launcher.") {
                    self.launch_saved_application(id.to_owned(), platform, false);
                }
            }
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
        ui::notification_layout(area, &model).area().is_some()
    }
}
