use super::*;
use i18n::LocalizedText;
use ui::NotificationPanelAction as Action;
use workspace_protocol::SessionId;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    id: u64,
    window: Option<SessionId>,
    source: LocalizedText,
    report: app::NotificationReport,
    log: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct NotificationHistory {
    entries: Vec<Entry>,
    sequence: u64,
    pub(super) pending_dismissals: BTreeMap<SessionId, Vec<u64>>,
    pub(super) displayed_report: Option<u64>,
    pub(super) header_report: Option<u64>,
    pub(super) header_override: Option<String>,
    pub(super) refresh_header: bool,
    pub(super) focus: Option<Action>,
    pub(super) pressed: Option<Action>,
}

impl NotificationHistory {
    pub(super) fn add(
        &mut self,
        window: Option<SessionId>,
        source: LocalizedText,
        report: app::NotificationReport,
        log: Option<PathBuf>,
    ) {
        self.sequence += 1;
        self.entries.push(Entry {
            id: self.sequence,
            window,
            source,
            report,
            log,
        });
    }

    pub(super) fn find_report(&self, window: Option<SessionId>, report: u64) -> Option<u64> {
        self.entries
            .iter()
            .rev()
            .find(|entry| entry.window == window && entry.report.id == report)
            .map(|entry| entry.id)
    }

    pub(super) fn open_panel(&mut self) {
        self.header_report = self.displayed_report;
        self.header_override = None;
        self.refresh_header = false;
        self.focus = None;
        self.pressed = None;
    }

    pub(super) fn panel(&self, can_open_logs: bool) -> ui::NotificationPanelModel {
        let cards = self
            .entries
            .iter()
            .rev()
            .map(|entry| ui::NotificationCard {
                id: entry.id,
                tone: entry.report.tone,
                source: entry.source.render_current(),
                title: entry
                    .report
                    .title
                    .as_ref()
                    .map(LocalizedText::render_current),
                message: entry.report.message.render_current(),
                open_log: can_open_logs && entry.log.is_some(),
            })
            .collect();
        ui::NotificationPanelModel {
            cards,
            focus: self.focus,
        }
    }
}

impl ShellSession {
    pub(super) fn collect_desktop_reports(&mut self) {
        if !self.window_owner.is_app() {
            for report in self.app.take_notification_reports() {
                self.notification_history.add(
                    None,
                    i18n::msg!("notification-source-desktop").into(),
                    report,
                    None,
                );
            }
        }
    }

    pub(super) fn previous_session_report(&mut self, crashed: bool, log: Option<PathBuf>) {
        let message = if crashed {
            i18n::msg!("notification-previous-crash")
        } else {
            i18n::msg!("notification-previous-unclean")
        };
        self.notification_history.add(
            None,
            i18n::msg!("notification-source-desktop").into(),
            app::NotificationReport {
                id: 0,
                tone: ui::NotificationTone::Critical,
                title: None,
                message: message.into(),
            },
            log,
        );
    }

    fn panel_layout(&self) -> Option<ui::NotificationPanelLayout> {
        let model = self.notification_active_modal_view_model()?;
        let panel = model.panel.as_ref()?;
        Some(ui::NotificationPanelLayout::new(
            self.shell_modal_area(),
            &model,
            panel,
        ))
    }

    pub(super) fn move_panel_focus(&mut self, back: bool) {
        if let Some(layout) = self.panel_layout() {
            let (focus, offset) = layout.move_focus(self.notification_history.focus, back);
            self.notification_history.focus = Some(focus);
            self.pages.common.notification_message_scroll = offset;
        }
    }

    pub(super) fn panel_mouse_command(&mut self, mouse: MouseInput) -> ShellCommand {
        let Some(layout) = self.panel_layout() else {
            return ShellCommand::CaptureOverlayInput;
        };
        let point = mouse.coordinates();
        let target = if layout.area.contains(point.into()) {
            layout.action_at(point)
        } else {
            Some(Action::Close)
        };
        match mouse.kind {
            ui::MouseEventKind::Down(PointerButton::Left) => {
                self.notification_history.pressed = target;
                self.notification_history.focus = target;
            }
            ui::MouseEventKind::Up(PointerButton::Left) => {
                if let Some(pressed) = self.notification_history.pressed.take()
                    && (target == Some(pressed)
                        || matches!(
                            (pressed, target),
                            (Action::Clear(_), Some(Action::Clear(_)))
                        ))
                {
                    return ShellCommand::NotificationPanelAction(pressed);
                }
            }
            ui::MouseEventKind::Moved if target == self.notification_history.pressed => {}
            ui::MouseEventKind::Scroll(ScrollDirection::Up | ScrollDirection::Down) => {
                self.notification_history.pressed = None;
                return ShellCommand::NotificationScrollMessage {
                    delta: if matches!(mouse.kind, ui::MouseEventKind::Scroll(ScrollDirection::Up))
                    {
                        -1
                    } else {
                        1
                    },
                    page: false,
                };
            }
            _ => self.notification_history.pressed = None,
        }
        ShellCommand::CaptureOverlayInput
    }

    pub(super) fn activate_panel_action(&mut self, action: Action) -> ShellAction {
        if !self.status_details_visible() {
            return ShellAction::Redraw;
        }
        match action {
            Action::Close => return self.activate_notification_action(0),
            Action::Open(id) => {
                if self.diagnostics_can_view_details()
                    && let Some(path) = self
                        .notification_history
                        .entries
                        .iter()
                        .find(|e| e.id == id)
                        .and_then(|e| e.log.clone())
                {
                    self.activate_notification_action(0);
                    if let Err(error) =
                        self.open_diagnostics_editor(EditorReloadPolicy::DiagnosticsReport { path })
                    {
                        self.notify_alert_with_tone(error, ui::NotificationTone::Error);
                    }
                }
            }
            Action::Dismiss(_) | Action::Clear(_) => {
                let history = &mut self.ui.notification_history;
                for entry in history.entries.extract_if(.., |entry| match action {
                    Action::Dismiss(id) => entry.id == id,
                    Action::Clear(through) => entry.id <= through,
                    _ => false,
                }) {
                    if history.header_report == Some(entry.id) {
                        history.refresh_header = true;
                    }
                    if entry.report.title.is_none() && entry.report.id != 0 {
                        if let Some(window) = entry.window {
                            history
                                .pending_dismissals
                                .entry(window)
                                .or_default()
                                .push(entry.report.id);
                        } else {
                            self.app.dismiss_notification_report(entry.report.id);
                        }
                    }
                }
                history.focus = None;
                history.pressed = None;
            }
        }
        ShellAction::Redraw
    }
}
