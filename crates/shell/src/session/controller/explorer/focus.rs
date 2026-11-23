use crate::session::*;
use ui::{ExplorerFocus, ExplorerSaveFocus};

impl ShellSession {
    pub(in crate::session) fn set_explorer_focus(&mut self, focus: ExplorerFocus) {
        if matches!(
            (self.pages.explorer.explorer_input_mode, focus),
            (ExplorerInputMode::Address, ExplorerFocus::Address)
                | (ExplorerInputMode::Search, ExplorerFocus::Search)
        ) {
            return;
        }
        if matches!(
            self.pages.explorer.explorer_input_mode,
            ExplorerInputMode::Address | ExplorerInputMode::Search
        ) {
            self.finish_explorer_input();
        }
        self.pages.explorer.explorer_focus = focus;
        if let ExplorerPurpose::EditorSaveAs { input, .. } =
            &mut self.pages.explorer.explorer_purpose
        {
            input.set_focused(focus == ExplorerFocus::SavePicker(ExplorerSaveFocus::Filename));
        }
    }

    pub(in crate::session) fn move_explorer_focus(&mut self, reverse: bool) {
        let mut model = self.to_explorer_view_model();
        model.address_editing = false;
        let layout = ui::explorer_layout(self.touch_main(), &model);
        let targets = layout.focus_targets(&model);
        if targets.is_empty() {
            return;
        }
        let index = targets.iter().position(|(focus, _)| *focus == model.focus);
        let next = match (reverse, index) {
            (true, Some(index)) => index.checked_sub(1).unwrap_or(targets.len() - 1),
            (false, Some(index)) => (index + 1) % targets.len(),
            (true, None) => targets.len() - 1,
            (false, None) => 0,
        };
        let (focus, _) = targets[next];
        self.set_explorer_focus(focus);
        match focus {
            ExplorerFocus::Search => self.prepare_explorer_input(ExplorerInputMode::Search),
            ExplorerFocus::Sidebar => self.pages.explorer.explorer_locations_scroll = None,
            ExplorerFocus::Files => {
                let _ = self.update_explorer_state(|state| {
                    state.viewport_follows_focus = true;
                });
            }
            _ => {}
        }
    }

    pub(in crate::session) fn activate_explorer_focus(&mut self, platform: &dyn Platform) {
        let model = self.to_explorer_view_model();
        let layout = ui::explorer_layout(self.touch_main(), &model);
        let Some((focus, area)) = layout
            .focus_targets(&model)
            .into_iter()
            .find(|(focus, _)| *focus == model.focus)
        else {
            return;
        };
        match focus {
            ExplorerFocus::Toolbar(action) => {
                self.activate_explorer_toolbar(action, (area.x, area.y), platform);
            }
            ExplorerFocus::Breadcrumb(index) => self.activate_explorer_breadcrumb(index, platform),
            ExplorerFocus::Address => self.begin_explorer_input(ExplorerInputMode::Address),
            ExplorerFocus::Search => self.begin_explorer_input(ExplorerInputMode::Search),
            ExplorerFocus::Sidebar => {
                if let Some(index) = model.quick_location_focus {
                    self.activate_explorer_quick_location(index, platform);
                }
            }
            ExplorerFocus::CancelOperation => {
                self.apply_explorer_command(ExplorerCommand::CancelOperation, platform);
            }
            ExplorerFocus::Files | ExplorerFocus::SavePicker(_) => {}
        }
    }

    pub(in crate::session) fn move_explorer_location_focus(&mut self, key: InputKey) {
        let model = self.to_explorer_view_model();
        if let Some(index) = super::explorer_quick_location_index(&model, key) {
            self.remember_explorer_location(index);
        }
    }

    pub(super) fn remember_explorer_location(&mut self, index: usize) {
        if let Some(location) = self
            .app
            .explorer_state()
            .and_then(|state| state.quick_locations.get(index))
        {
            self.pages.explorer.explorer_location_focus = Some(location.id.clone());
            self.pages.explorer.explorer_locations_scroll = None;
        }
    }
}
