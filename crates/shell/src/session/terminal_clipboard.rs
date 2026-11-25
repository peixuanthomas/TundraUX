use super::*;
use ui::components::{ComponentEvent, ContextMenu, ContextMenuItem};
use ui::{CommandLineTerminalSnapshot, Point, TerminalClipboardView, TerminalSelection};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct TerminalClipboard {
    view: TerminalClipboardView,
    anchor: Option<Point>,
}

pub(super) enum TerminalClipboardAction {
    Consumed,
    Copy(String),
    Paste,
    Interrupt,
}

impl TerminalClipboard {
    pub(super) fn view(&self) -> TerminalClipboardView {
        self.view.clone()
    }

    pub(super) fn menu_open(&self) -> bool {
        self.view.menu.is_some()
    }

    pub(super) fn reset_pointer(&mut self) {
        self.anchor = None;
        self.view.menu = None;
    }

    pub(super) fn clear(&mut self) {
        self.reset_pointer();
        self.view.selection = None;
    }

    pub(super) fn update_snapshot(
        &mut self,
        previous: &CommandLineTerminalSnapshot,
        next: &CommandLineTerminalSnapshot,
    ) {
        if previous.columns != next.columns
            || previous.rows != next.rows
            || previous.scrollback_offset != next.scrollback_offset
            || self
                .view
                .selection
                .is_some_and(|selection| selection.text(previous) != selection.text(next))
        {
            self.clear();
        }
    }

    pub(super) fn handle_shortcut(
        &mut self,
        input: &InputEvent,
        content: Rect,
        snapshot: &CommandLineTerminalSnapshot,
        running: bool,
    ) -> Option<TerminalClipboardAction> {
        use TerminalClipboardAction as Action;
        if let InputEvent::Key(key) = input {
            if key.modifiers.is_control_only() || key.modifiers == ui::KeyModifiers::CTRL_SHIFT {
                let action = match key.key {
                    InputKey::Char('c' | 'C') => Some(if key.phase == InputPhase::Press {
                        self.copy_selection(snapshot)
                    } else {
                        Action::Consumed
                    }),
                    InputKey::Char('v' | 'V') => {
                        Some(if running && key.phase == InputPhase::Press {
                            Action::Paste
                        } else {
                            Action::Consumed
                        })
                    }
                    InputKey::Char('\\') if !key.modifiers.shift => {
                        Some(if running && key.phase == InputPhase::Press {
                            Action::Interrupt
                        } else {
                            Action::Consumed
                        })
                    }
                    _ => None,
                };
                if action.is_some() {
                    return action;
                }
            }
            if key.key == InputKey::F(10) && key.modifiers == ui::KeyModifiers::SHIFT {
                if key.phase == InputPhase::Press {
                    self.open_menu(Point::new(content.x, content.y), running);
                }
                return Some(Action::Consumed);
            }
        }
        let (anchor, menu) = self.view.menu.as_mut()?;
        let area = menu.preferred_area(anchor.column, anchor.row, content);
        let mut input = input.clone();
        if let InputEvent::Key(key) = &mut input {
            match key.key {
                InputKey::BackTab => key.key = InputKey::Up,
                InputKey::Tab if key.modifiers.shift => key.key = InputKey::Up,
                InputKey::Tab => key.key = InputKey::Down,
                _ => {}
            }
            if key.phase != InputPhase::Press && key.key == InputKey::Enter {
                return Some(Action::Consumed);
            }
        }
        let event = menu.handle_event(input, area);
        if !menu.open {
            self.view.menu = None;
        }
        Some(match event {
            ComponentEvent::Activated(id) => match id.as_str() {
                "terminal.copy" => self.copy_selection(snapshot),
                "terminal.paste" if running => Action::Paste,
                "terminal.interrupt" if running => Action::Interrupt,
                _ => Action::Consumed,
            },
            _ => Action::Consumed,
        })
    }

    pub(super) fn captures_pointer(&self, mouse: ui::MouseEvent) -> bool {
        mouse.modifiers.shift
            || (self.anchor.is_some()
                && matches!(
                    mouse.kind,
                    ui::MouseEventKind::Drag(PointerButton::Left)
                        | ui::MouseEventKind::Up(PointerButton::Left)
                ))
    }

    pub(super) fn handle_pointer(
        &mut self,
        mouse: ui::MouseEvent,
        content: Rect,
        running: bool,
    ) -> bool {
        if content.is_empty() {
            return false;
        }
        let inside = content.contains((mouse.column(), mouse.row()).into());
        let point = Point::new(
            mouse.column().clamp(content.x, content.right() - 1) - content.x,
            mouse.row().clamp(content.y, content.bottom() - 1) - content.y,
        );
        match mouse.kind {
            ui::MouseEventKind::Down(PointerButton::Right) if inside => {
                self.open_menu(mouse.position, running);
            }
            ui::MouseEventKind::Down(PointerButton::Left) if inside => {
                self.anchor = Some(point);
                self.view.selection = None;
            }
            ui::MouseEventKind::Drag(PointerButton::Left)
            | ui::MouseEventKind::Up(PointerButton::Left)
                if let Some(anchor) = self.anchor =>
            {
                self.view.selection =
                    (anchor != point).then(|| TerminalSelection::new(anchor, point));
                if matches!(mouse.kind, ui::MouseEventKind::Up(_)) {
                    self.anchor = None;
                }
            }
            _ => return false,
        }
        true
    }

    fn open_menu(&mut self, anchor: Point, running: bool) {
        let mut menu = ContextMenu::new(
            "terminal.clipboard",
            vec![
                ContextMenuItem::new("terminal.copy", i18n::tr!("shell-terminal-menu-copy"))
                    .disabled(self.view.selection.is_none()),
                ContextMenuItem::new("terminal.paste", i18n::tr!("shell-terminal-menu-paste"))
                    .disabled(!running),
                ContextMenuItem::new(
                    "terminal.interrupt",
                    i18n::tr!("shell-terminal-menu-interrupt"),
                )
                .disabled(!running),
            ],
        );
        menu.open();
        menu.set_selected(menu.items.iter().position(|item| !item.disabled));
        self.view.menu = Some((anchor, menu));
        self.anchor = None;
    }

    fn copy_selection(&self, snapshot: &CommandLineTerminalSnapshot) -> TerminalClipboardAction {
        self.view
            .selection
            .map_or(TerminalClipboardAction::Consumed, |selection| {
                TerminalClipboardAction::Copy(selection.text(snapshot))
            })
    }
}

impl ShellSession {
    pub(super) fn perform_terminal_clipboard_action(
        &mut self,
        action: TerminalClipboardAction,
        process: Option<&PtyProcess>,
    ) {
        use TerminalClipboardAction as Action;
        let clipboard = self.text_clipboard();
        let result = match (action, process) {
            (Action::Copy(text), _) => clipboard.write(text).inspect(|()| {
                self.notify_status(i18n::msg!("shell-copied"));
            }),
            (Action::Paste, Some(process)) => clipboard.read().and_then(|text| {
                if text.is_empty() {
                    self.notify_status(i18n::msg!("shell-clipboard-is-empty"));
                    Ok(())
                } else {
                    process.send_paste(&text)
                }
            }),
            (Action::Interrupt, Some(process)) => {
                if let Err(error) = process.send(&TerminalInput::CtrlC) {
                    self.notify_status(i18n::msg!(
                        "shell-terminal-input-failed",
                        reason = error.to_string()
                    ));
                }
                return;
            }
            _ => return,
        };
        if let Err(error) = result {
            self.notify_status(i18n::msg!(
                "shell-text-clipboard-error",
                error = error.to_string()
            ));
        }
    }
}
