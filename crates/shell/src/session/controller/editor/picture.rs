use crate::session::*;

impl ShellSession {
    pub(super) fn change_editor_picture_zoom(&mut self, zoom_in: bool) {
        if let Some(zoom) = &mut self.pages.editor.editor_picture_zoom {
            zoom.change(zoom_in);
            self.clear_editor_scrollbar_drag();
        }
    }

    pub(super) fn handle_editor_picture_key(&mut self, key: &KeyInput) -> bool {
        if key.has_non_shift_modifier()
            || self.pages.editor.editor_picture_zoom.is_none()
            || !self
                .app
                .editor_state()
                .is_some_and(|state| state.mode == app::editor::EditorMode::Rich)
        {
            return false;
        }
        match key.key {
            InputKey::Char('+' | '=') => self.change_editor_picture_zoom(true),
            InputKey::Char('-') => self.change_editor_picture_zoom(false),
            _ => {
                return super::preview_pan_direction(key)
                    .is_some_and(|delta| self.pan_editor_picture(delta));
            }
        }
        true
    }

    pub(super) fn pan_editor_picture(&mut self, delta: [i32; 2]) -> bool {
        if !self
            .pages
            .editor
            .editor_picture_zoom
            .is_some_and(ui::EditorPreviewZoom::is_zoomed)
        {
            return false;
        }
        let Some(window) = self.editor_current_layout().picture_window else {
            return false;
        };
        self.pages
            .editor
            .editor_picture_zoom
            .as_mut()
            .unwrap()
            .pan(window, delta);
        true
    }
}
