use crate::session::*;
use ui::components::{ComponentEvent, ContextMenu, ContextMenuItem};

impl ShellSession {
    pub(in crate::session) fn load_home_order(&mut self) {
        let Some(session) = self.app.auth_session() else {
            return;
        };
        if self.pages.home.order_user_id.as_deref() == Some(&session.user_id) {
            return;
        }
        let Some(storage) = self.storage_manager.as_ref() else {
            return;
        };
        let user_id = session.user_id.clone();
        let preferences = storage.load_state().ok();
        let load = |name| {
            preferences
                .as_ref()?
                .values
                .get(&format!("home.{name}.{user_id}"))
                .and_then(|value| serde_json::from_str::<Vec<String>>(value).ok())
        };
        self.pages.home.order = load("order").unwrap_or_default();
        self.pages.home.shortcuts = load("shortcuts");
        self.pages.home.order_user_id = Some(user_id);
        self.pages.home.editing = false;
        self.pages.home.drag = None;
    }

    pub(in crate::session) fn toggle_home_editing(&mut self) {
        if self.user_home_entries().is_empty() {
            return;
        }
        self.pages.home.editing = !self.pages.home.editing;
        self.pages.home.drag = None;
        self.interaction.button_capture = None;
    }

    pub(in crate::session) fn move_home_entry(&mut self, delta: isize) {
        if !self.pages.home.editing {
            return;
        }
        let source = self.selected_home_entry_index();
        let destination = source
            .saturating_add_signed(delta)
            .min(self.user_home_entries().len().saturating_sub(1));
        self.reorder_home_entry(source, destination);
    }

    fn reorder_home_entry(&mut self, source: usize, destination: usize) {
        if source == destination {
            return;
        }
        let mut order = self
            .user_home_entries()
            .iter()
            .map(|entry| entry.icon_identity().to_owned())
            .collect::<Vec<_>>();
        let entry = order.remove(source);
        order.insert(destination, entry);
        if self.save_home_preference("order", &order) {
            self.pages.home.order = order;
            self.select_home_entry(destination);
        }
    }

    fn save_home_preference(&mut self, name: &str, entries: &[String]) -> bool {
        if let Some(storage) = self.storage_manager.as_ref()
            && let Some(session) = self.app.auth_session()
        {
            let result = storage.load_state().and_then(|mut state| {
                state.values.insert(
                    format!("home.{name}.{}", session.user_id),
                    serde_json::to_string(entries).expect("home icon IDs serialize"),
                );
                storage.save_state(&state)
            });
            if let Err(error) = result {
                self.notify_alert_with_tone(
                    i18n::msg!(
                        "shell-could-not-save-arg1-error",
                        arg1 = i18n::tr!("ui-home-home"),
                        error = error.to_string()
                    ),
                    ui::NotificationTone::Error,
                );
                return false;
            }
        }
        true
    }

    pub(in crate::session) fn set_home_shortcut(&mut self, key: String, show: bool) {
        self.load_home_order();
        let mut shortcuts = self.pages.home.shortcuts.clone().unwrap_or_else(|| {
            self.user_home_entries()
                .iter()
                .map(|entry| entry.icon_identity().to_owned())
                .collect()
        });
        if show && !shortcuts.contains(&key) {
            shortcuts.push(key);
        } else if !show {
            shortcuts.retain(|entry| entry != &key);
        }
        if self.save_home_preference("shortcuts", &shortcuts) {
            self.pages.home.shortcuts = Some(shortcuts);
            self.sync_home_entry_selection();
            self.refresh_hit_map();
        }
    }

    pub(in crate::session) fn open_home_context_menu(&mut self, anchor: CellPosition) {
        let Some(key) = self.home_entry_index_at(anchor).and_then(|index| {
            self.user_home_entries()
                .get(index)
                .map(|entry| entry.icon_identity().to_owned())
        }) else {
            return;
        };
        if key == "launcher" {
            return;
        }
        let mut menu = ContextMenu::new(
            "home.menu",
            vec![ContextMenuItem::new(
                "remove",
                i18n::tr!("ui-home-remove-shortcut"),
            )],
        );
        menu.open();
        self.pages.home.context_menu = Some((key, menu));
        self.pages.common.active_popup = Some(ShellPopup {
            owner: Some(ShellComponent::Home),
            anchor,
        });
        self.refresh_hit_map();
    }

    pub(in crate::session) fn home_context_menu(
        &self,
        bounds: Rect,
    ) -> Option<(&ContextMenu, Rect)> {
        let popup = self
            .active_popup()
            .filter(|popup| popup.owner == Some(ShellComponent::Home))?;
        let (_, menu) = self.pages.home.context_menu.as_ref()?;
        Some((
            menu,
            menu.preferred_area(popup.anchor.0, popup.anchor.1, bounds),
        ))
    }

    pub(in crate::session) fn handle_home_context_input(&mut self, input: InputEvent) {
        let Some((_, area)) = self.home_context_menu(self.touch_main()) else {
            return;
        };
        let (key, menu) = self
            .pages
            .home
            .context_menu
            .as_mut()
            .expect("open Home menu");
        let event = menu.handle_event(input, area);
        let remove = matches!(event, ComponentEvent::Activated(_)).then(|| key.clone());
        if !menu.open {
            self.pages.home.context_menu = None;
            self.pages.common.active_popup = None;
            self.refresh_hit_map();
        }
        if let Some(key) = remove {
            self.set_home_shortcut(key, false);
        }
    }

    pub(in crate::session) fn begin_home_drag(&mut self, index: usize) {
        if let Some(capture) = &mut self.interaction.button_capture {
            capture.activate_on_release = false;
        }
        let entries = self.user_home_entries();
        if entries.len() > 1 {
            self.pages.home.drag = entries.get(index).map(|entry| HomeDragState {
                entry_id: entry.icon_identity().to_owned(),
                target: None,
            });
        }
    }

    pub(in crate::session) fn update_home_drag(&mut self, coordinates: CellPosition) {
        let target = self
            .home_content_layout()
            .and_then(|layout| layout.drop_target(coordinates));
        if let Some(drag) = self.pages.home.drag.as_mut() {
            drag.target = target;
        }
    }

    pub(in crate::session) fn drop_home_drag(&mut self, coordinates: CellPosition) {
        let Some(drag) = self.pages.home.drag.take() else {
            return;
        };
        if !self.pages.home.editing || drag.target.is_none() {
            return;
        }
        let Some(target) = self
            .home_content_layout()
            .and_then(|layout| layout.drop_target(coordinates))
        else {
            return;
        };
        let entries = self.user_home_entries();
        let Some(source) = entries
            .iter()
            .position(|entry| entry.icon_identity() == drag.entry_id)
        else {
            return;
        };
        let boundary = target.insertion_index();
        let destination = boundary - usize::from(source < boundary);
        self.reorder_home_entry(source, destination);
    }
}
