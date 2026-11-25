use super::*;

impl ShellSession {
    pub(in crate::session) fn page_keyboard_identity(&self) -> Option<String> {
        let overlays = self.interactive_overlays();
        let overlay = overlays.last();
        if let Some(overlay) = overlay {
            let accepts_text = match overlay.component() {
                Some(ShellComponent::Launcher) => {
                    self.pages
                        .launcher
                        .entry_editor
                        .as_ref()
                        .is_some_and(|editor| editor.selected < editor.inputs.len())
                        || self
                            .pages
                            .launcher
                            .launcher_run_settings
                            .as_ref()
                            .is_some_and(|state| {
                                state.embedded
                                    && if state.custom {
                                        state.selected < 2
                                    } else {
                                        matches!(state.selected, 1 | 2)
                                    }
                            })
                }
                Some(ShellComponent::Explorer) => matches!(
                    self.resolved_explorer_overlay(),
                    Some(
                        overlays::ResolvedExplorerOverlay::Input(_)
                            | overlays::ResolvedExplorerOverlay::Archive
                    )
                ),
                Some(ShellComponent::Settings) => {
                    (overlay.id.starts_with("settings-editor:")
                        && (overlay.id != "settings-editor:animation"
                            || self
                                .pages
                                .settings
                                .settings_state
                                .as_ref()
                                .and_then(|state| state.animation_editor.as_ref())
                                .is_some_and(|editor| {
                                    editor.form.fields.iter().any(|field| !field.disabled)
                                })))
                        || self
                            .pages
                            .settings
                            .settings_state
                            .as_ref()
                            .and_then(|state| state.picker.as_ref())
                            .is_some()
                }
                Some(ShellComponent::SystemStatus) => {
                    self.pages.system_status.system_status_add_picker.is_some()
                        || self.pages.system_status.system_status_size_picker.is_some()
                }
                Some(ShellComponent::SetupCustomColorDialog | ShellComponent::ClockCreateInput) => {
                    true
                }
                _ => false,
            };
            return accepts_text.then(|| overlay.id.clone());
        }
        match self.active_screen() {
            ShellScreen::Editor => Some(format!("{:?}", self.active_screen())),
            ShellScreen::Launcher if self.pages.launcher.search.state.focused => {
                Some("launcher-search".into())
            }
            ShellScreen::Explorer => Some(format!(
                "explorer:{:?}",
                self.pages.explorer.explorer_input_mode
            )),
            _ => None,
        }
    }

    pub(super) fn page_keyboard_target(&self) -> String {
        let mut target = format!("{:?}", self.pages.common.focused_component);
        if self.config_editor_form_visible() {
            target.push_str(&self.management_keyboard_target());
        }
        match self.active_screen() {
            ShellScreen::Explorer => {
                target.push_str(&format!(":{:?}", self.pages.explorer.explorer_focus));
            }
            ShellScreen::Launcher => {
                if self.pages.launcher.search.state.focused {
                    target.push_str(":search");
                }
                if let Some(editor) = &self.pages.launcher.entry_editor {
                    target.push_str(&format!(":edit:{}:{}", editor.id, editor.selected));
                }
                if let Some(state) = &self.pages.launcher.launcher_run_settings {
                    target.push_str(&format!(":{}:{}", state.id, state.selected));
                }
            }
            ShellScreen::Editor => {
                target.push_str(&format!(":{:?}", self.pages.editor.editor_focus));
                if let Some(search) = &self.pages.editor.editor_search_bar {
                    target.push_str(&format!(":{:?}", search.focus));
                }
            }
            ShellScreen::Management => {
                target.push_str(&self.management_keyboard_target());
            }
            ShellScreen::Settings => {
                if let Some(editor) = self
                    .pages
                    .settings
                    .settings_state
                    .as_ref()
                    .and_then(|state| state.animation_editor.as_ref())
                {
                    target.push_str(&format!(":animation:{}", editor.form.selected));
                }
            }
            ShellScreen::Clock => {
                if let Some(form) = &self.pages.clock.clock_create_state {
                    target.push_str(&format!(":{:?}:{}", form.focus, form.active_field));
                }
            }
            _ => {}
        }
        target
    }

    pub(super) fn focus_page_keyboard_input(&mut self) {
        match self.active_screen() {
            ShellScreen::Launcher => {
                self.pages.common.focused_component = ShellComponent::Launcher;
                if self.pages.launcher.entry_editor.is_none()
                    && self.pages.launcher.launcher_run_settings.is_none()
                {
                    self.pages.launcher.search.set_focused(true);
                }
                if let Some(editor) = &mut self.pages.launcher.entry_editor {
                    editor.selected = editor.selected.min(editor.inputs.len().saturating_sub(1));
                }
                if let Some(state) = &mut self.pages.launcher.launcher_run_settings
                    && state.embedded
                    && !(if state.custom {
                        state.selected < 2
                    } else {
                        matches!(state.selected, 1 | 2)
                    })
                {
                    state.selected = 1;
                }
            }
            ShellScreen::Explorer => {
                if self.pages.explorer.explorer_input_mode == ExplorerInputMode::Browse {
                    if matches!(
                        self.pages.explorer.explorer_purpose,
                        ExplorerPurpose::EditorSaveAs { .. }
                    ) {
                        self.set_explorer_focus(ui::ExplorerFocus::SavePicker(
                            ui::ExplorerSaveFocus::Filename,
                        ));
                    } else {
                        self.begin_explorer_input(ExplorerInputMode::Search);
                    }
                }
                self.pages.explorer.explorer_overlay_selection = 0;
                self.pages.common.focused_component = ShellComponent::Explorer;
            }
            ShellScreen::Editor => {
                self.pages.common.focused_component = ShellComponent::Editor;
                if self
                    .pages
                    .editor
                    .editor_search_bar
                    .as_ref()
                    .is_none_or(|search| search.focus.is_none())
                {
                    self.pages.editor.editor_focus = ui::EditorFocus::Canvas;
                    self.pages.editor.editor_selected_toolbar_action = None;
                }
            }
            ShellScreen::Clock => {
                if let Some(form) = &mut self.pages.clock.clock_create_state {
                    form.focus = ui::ClockCreateDialogFocus::Input;
                    self.pages.common.focused_component = ShellComponent::ClockCreateInput;
                }
            }
            ShellScreen::Settings => {
                if let Some(editor) = self
                    .pages
                    .settings
                    .settings_state
                    .as_mut()
                    .and_then(|state| state.number_editor.as_mut())
                {
                    editor.selected_index = 0;
                } else if let Some(editor) = self
                    .pages
                    .settings
                    .settings_state
                    .as_mut()
                    .and_then(|state| state.animation_editor.as_mut())
                {
                    editor.form.selected = editor.form.selected.min(editor.form.fields.len() - 1);
                }
            }
            _ => {}
        }
    }

    pub(in crate::session) fn paste_native_text(
        &mut self,
        value: &str,
        platform: &dyn Platform,
        at: Instant,
    ) {
        if self.page_keyboard_identity().is_none() {
            return;
        }
        let page = self.active_page_id();
        let owner = self.page_keyboard_identity();
        // Pasted line breaks must never submit a password or a management form.
        for character in value.chars().filter(|character| !character.is_control()) {
            if self.active_page_id() != page || self.page_keyboard_identity() != owner {
                break;
            }
            let input = InputEvent::Key(KeyInput::new(InputKey::Char(character)));
            let (target, command) = self.route_key_input(match &input {
                InputEvent::Key(key) => key,
                _ => unreachable!(),
            });
            let _ = self.apply_routed_event_once(
                RoutedEvent {
                    input,
                    target,
                    command,
                },
                platform,
                at,
            );
        }
    }
}
