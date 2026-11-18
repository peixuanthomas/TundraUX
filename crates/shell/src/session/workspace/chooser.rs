use super::*;
use ui::components::{ComponentEvent, SelectionList, SelectionListItem};

#[derive(Clone)]
enum Choice {
    Window(SessionId),
    Application(WindowLaunch),
    Launcher(String),
}

pub(super) struct WindowChooser {
    pub(super) source: SessionId,
    pub(super) list: SelectionList,
    pub(super) awaiting_launcher: bool,
    choices: HashMap<String, Choice>,
    area: Rect,
}

impl Workspace {
    pub(super) fn can_split(&self, id: SessionId, axis: WorkspaceSplitMode) -> bool {
        self.windows
            .get(&id)
            .is_some_and(|window| window.close == CloseProgress::Idle)
            && self
                .selection
                .tree
                .as_ref()
                .is_some_and(|tree| self.layout.can_split(tree, id, axis))
    }

    pub(super) fn begin_split(
        &mut self,
        source: SessionId,
        axis: WorkspaceSplitMode,
        state: &ShellSession,
    ) {
        if !self.can_split(source, axis) {
            return;
        }
        self.close_split_menu();
        self.prefix.clear();
        if !self.selection.split(source, axis) {
            return;
        }
        let mut items = Vec::new();
        let mut choices = HashMap::new();
        for (&id, window) in &self.windows {
            if id == source || self.selection.includes(id) {
                continue;
            }
            let key = format!("window.{id}");
            items.push(
                SelectionListItem::new(key.as_str(), window.launch.title())
                    .with_group(i18n::tr!("ui-workspace-minimized-windows"))
                    .disabled(window.close != CloseProgress::Idle),
            );
            choices.insert(key, Choice::Window(id));
        }
        let group = i18n::tr!("ui-workspace-start-application");
        for entry in state.default_home_entries() {
            let Some(launch) = WindowLaunch::for_builtin(entry.icon_identity()) else {
                continue;
            };
            let key = format!("application.{}", entry.icon_identity());
            items.push(
                SelectionListItem::new(key.as_str(), entry.label)
                    .with_keywords([entry.description])
                    .with_group(&group),
            );
            choices.insert(key, Choice::Application(launch));
        }
        for app in state.available_built_in_applications() {
            let Some(launch) = WindowLaunch::for_builtin(app.id.trim_start_matches("builtin."))
            else {
                continue;
            };
            if choices.values().any(
                |choice| matches!(choice, Choice::Application(existing) if *existing == launch),
            ) {
                continue;
            }
            items.push(
                SelectionListItem::new(app.id, app.localized_name().render_current())
                    .with_keywords([app.localized_description().render_current()])
                    .with_group(&group),
            );
            choices.insert(app.id.to_owned(), Choice::Application(launch));
        }
        for entry in &state.app.storage_config().launcher.entries {
            let managed = entry.custom.is_some() || entry.terminal.is_some();
            let key = format!("launcher.{}", entry.id);
            items.push(
                SelectionListItem::new(key.as_str(), app::launcher::launcher_entry_name(entry))
                    .with_hint(entry.path.clone())
                    .with_group(&group)
                    .disabled(!managed || !state.can_execute_command_line()),
            );
            choices.insert(key, Choice::Launcher(entry.id.clone()));
        }
        let mut list = SelectionList::new("workspace.choose", items)
            .titled(i18n::tr!("ui-workspace-choose-application"))
            .with_placeholder(i18n::tr!("ui-workspace-search"))
            .with_scrollbar();
        list.open();
        self.chooser = Some(WindowChooser {
            source,
            list,
            choices,
            area: Rect::default(),
            awaiting_launcher: false,
        });
        self.cancel_capture();
        self.relayout(
            Rect::new(0, 0, self.width, self.height),
            self.layout.available,
            false,
        );
        self.refresh_chooser_area();
    }

    pub(super) fn cancel_chooser(&mut self) {
        if self.chooser.take().is_some() {
            self.selection.cancel_add();
            self.cancel_capture();
        }
    }

    pub(super) fn chooser_input(
        &mut self,
        input: &InputEvent,
        state: &mut ShellSession,
    ) -> io::Result<()> {
        let chooser = self.chooser.as_mut().expect("window chooser");
        if chooser.awaiting_launcher {
            return Ok(());
        }
        let event = chooser.list.handle_event(input.clone(), chooser.area);
        if let ComponentEvent::Activated(id) = event {
            let choice = chooser.choices.get(id.as_str()).cloned();
            match choice {
                Some(Choice::Window(id))
                    if self
                        .windows
                        .get(&id)
                        .is_some_and(|window| window.close == CloseProgress::Idle) =>
                {
                    self.selection.open(id);
                    self.chooser = None;
                    self.cancel_capture();
                }
                Some(Choice::Application(launch)) => {
                    if let Err(error) = self.open(launch, state.auth_session()) {
                        self.cancel_chooser();
                        state.notify_alert_with_tone(
                            i18n::msg!("workspace-launch-failed", reason = error.to_string()),
                            ui::NotificationTone::Error,
                        );
                    }
                }
                Some(Choice::Launcher(id)) => {
                    self.chooser.as_mut().unwrap().awaiting_launcher = true;
                    state.open_taskbar_launcher_session(
                        LauncherCommand::RequestLaunch(id),
                        platform::native_platform().as_ref(),
                    );
                }
                _ => self.cancel_chooser(),
            }
        } else if !chooser.list.open {
            self.cancel_chooser();
        }
        Ok(())
    }

    pub(super) fn refresh_chooser_area(&mut self) {
        if let Some(pane) = self
            .layout
            .panes
            .iter()
            .find(|pane| pane.id == SessionId(0))
            && let Some(chooser) = &mut self.chooser
        {
            if chooser.area != pane.area {
                chooser.list.cancel_pointer();
            }
            chooser.area = pane.area;
        }
    }

    pub(super) fn render_chooser(
        &mut self,
        frame: &mut Frame<'_>,
        context: &ui::RenderContext,
        capture: Option<&ui::automation::FrameCapture>,
    ) {
        let chooser = self.chooser.as_ref().expect("window chooser");
        ui::dim_modal_backdrop(frame.buffer_mut(), self.layout.available, context);
        if let Some(capture) = capture {
            capture.set_group("workspace-chooser", "dialog", chooser.area, false);
        }
        chooser
            .list
            .render_with_context(chooser.area, frame.buffer_mut(), context);
    }
}
