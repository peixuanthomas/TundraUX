use crate::{InputKey, InputPhase, KeyInput};
use std::collections::HashMap;
use ui::{WorkspaceLayout, WorkspaceSplitMode, WorkspaceTree};
use workspace_protocol::SessionId;

#[derive(Default)]
pub(super) struct WindowSelection {
    pub(super) tree: Option<WorkspaceTree>,
    saved: HashMap<SessionId, WorkspaceTree>,
    focused: Option<SessionId>,
    pending_source: Option<SessionId>,
    opened_from: HashMap<SessionId, WindowReturn>,
}

struct WindowReturn {
    source: Option<SessionId>,
    covered: Option<SessionId>,
}

impl WindowSelection {
    pub(super) fn focused(&self) -> Option<SessionId> {
        self.focused
    }

    pub(super) fn includes(&self, id: SessionId) -> bool {
        self.tree
            .as_ref()
            .is_some_and(|tree| tree.ids().contains(&id))
    }

    pub(super) fn visible(&self) -> Vec<SessionId> {
        self.tree.as_ref().map_or_else(Vec::new, WorkspaceTree::ids)
    }

    pub(super) fn open(&mut self, id: SessionId) {
        if self.pending_source.take().is_some() {
            self.tree
                .as_mut()
                .expect("pending split")
                .replace(SessionId(0), id);
            self.saved.remove(&id);
            self.focused = Some(id);
        } else {
            self.switch(id);
        }
    }

    pub(super) fn open_from(&mut self, source: SessionId, id: SessionId, layout: &WorkspaceLayout) {
        self.cancel_add();
        let covered = layout
            .panes
            .iter()
            .map(|pane| pane.id)
            .find(|&pane| pane != source && self.includes(pane));
        let covered = if let Some(covered) = covered {
            self.replace(covered, id);
            self.focused = Some(id);
            Some(covered)
        } else if [WorkspaceSplitMode::LeftRight, WorkspaceSplitMode::TopBottom]
            .into_iter()
            .any(|axis| {
                self.tree
                    .as_ref()
                    .is_some_and(|tree| layout.can_split(tree, source, axis))
                    && self.split(source, axis)
            })
        {
            self.open(id);
            None
        } else {
            self.switch(id);
            Some(source)
        };
        self.opened_from.insert(
            id,
            WindowReturn {
                source: Some(source),
                covered,
            },
        );
    }

    pub(super) fn opener(&self, id: SessionId) -> Option<SessionId> {
        self.opened_from.get(&id)?.source
    }

    pub(super) fn switch(&mut self, id: SessionId) {
        self.cancel_add();
        if !self.includes(id) {
            if let Some(focused) = self.focused {
                self.replace(focused, id);
            } else {
                self.tree = Some(WorkspaceTree::Window(id));
            }
        }
        self.focused = Some(id);
    }

    fn replace(&mut self, previous: SessionId, next: SessionId) {
        let tree = self.tree.as_mut().expect("visible window");
        self.saved.insert(previous, tree.clone());
        tree.replace(previous, next);
    }

    pub(super) fn activate(&mut self, id: SessionId, layout: &WorkspaceLayout) {
        if !self.includes(id)
            && let Some(covered) = self.opened_from.get(&id).and_then(|window| window.covered)
            && self.includes(covered)
        {
            self.cancel_add();
            self.replace(covered, id);
            self.focused = Some(id);
            return;
        }
        if !self.includes(id)
            && let (Some(tree), Some(previous)) = (&self.tree, self.saved.get(&id))
            && let Some(restored) = tree.restore(previous, id)
            && layout.fits(&restored)
        {
            self.tree = Some(restored);
            self.saved.remove(&id);
        }
        self.switch(id);
    }

    pub(super) fn restore(&mut self, id: SessionId, layout: &WorkspaceLayout) {
        if self.pending_source.is_some() {
            self.open(id);
        } else {
            self.activate(id, layout);
        }
    }

    pub(super) fn focus(&mut self, id: SessionId) {
        if self.includes(id) && id != SessionId(0) {
            self.focused = Some(id);
        }
    }

    pub(super) fn split(&mut self, id: SessionId, axis: WorkspaceSplitMode) -> bool {
        if self
            .tree
            .as_mut()
            .is_some_and(|tree| tree.split(id, SessionId(0), axis))
        {
            self.pending_source = Some(id);
            self.focused = Some(id);
            true
        } else {
            false
        }
    }

    pub(super) fn cancel_add(&mut self) -> bool {
        let Some(source) = self.pending_source.take() else {
            return false;
        };
        self.tree = self
            .tree
            .as_ref()
            .and_then(|tree| tree.without(SessionId(0)));
        self.focus(source);
        true
    }

    pub(super) fn minimize(&mut self, id: SessionId) {
        if !self.includes(id) {
            return;
        }
        self.cancel_add();
        let Some(tree) = &self.tree else {
            return;
        };
        let ids = tree.ids();
        let Some(index) = ids.iter().position(|candidate| *candidate == id) else {
            return;
        };
        let covered = self
            .opened_from
            .get(&id)
            .and_then(|window| window.covered)
            .filter(|covered| !ids.contains(covered));
        let source = self.opener(id);
        if let Some(covered) = covered {
            self.replace(id, covered);
        } else {
            self.saved.insert(id, tree.clone());
            self.tree = tree.without(id);
        }
        if self.focused == Some(id) {
            self.focused = covered.or_else(|| {
                ids[index + 1..]
                    .iter()
                    .chain(ids[..index].iter().rev())
                    .copied()
                    .next()
            });
            if let Some(source) = source {
                self.focus(source);
            }
        }
    }

    pub(super) fn remove(&mut self, id: SessionId) {
        self.minimize(id);
        self.saved.remove(&id);
        self.opened_from.remove(&id);
        for window in self.opened_from.values_mut() {
            if window.source == Some(id) {
                window.source = None;
            }
            if window.covered == Some(id) {
                window.covered = None;
            }
        }
    }

    pub(super) fn fullscreen(&mut self, id: SessionId) {
        self.pending_source = None;
        self.saved.clear();
        self.tree = Some(WorkspaceTree::Window(id));
        self.focused = Some(id);
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum WindowShortcut {
    Pass,
    Consume,
    SendPrefix(KeyInput),
    Background,
    Panel,
    Notifications,
    Split(ui::WorkspaceSplitMode),
    Maximize,
    MoveFocus(i16, i16),
    Close,
    Kill,
}

#[derive(Default)]
pub(super) struct WindowPrefix(Option<KeyInput>);

impl WindowPrefix {
    pub(super) fn clear(&mut self) {
        self.0 = None;
    }

    pub(super) fn route(&mut self, key: &KeyInput) -> WindowShortcut {
        let prefix =
            key.modifiers.is_control_only() && matches!(key.key, InputKey::Char('w' | 'W'));
        if key.phase != InputPhase::Press {
            return if prefix || self.0.is_some() {
                WindowShortcut::Consume
            } else {
                WindowShortcut::Pass
            };
        }
        let Some(first) = self.0.take() else {
            if prefix {
                self.0 = Some(key.clone());
                return WindowShortcut::Consume;
            }
            return WindowShortcut::Pass;
        };
        if prefix {
            return WindowShortcut::SendPrefix(first);
        }
        if key.has_non_shift_modifier() {
            return WindowShortcut::Pass;
        }
        match key.key {
            InputKey::Char('n' | 'N') => WindowShortcut::Notifications,
            InputKey::Char('b' | 'B') => WindowShortcut::Background,
            InputKey::Char('w' | 'W') => WindowShortcut::Panel,
            InputKey::Char('v' | 'V') => WindowShortcut::Split(ui::WorkspaceSplitMode::LeftRight),
            InputKey::Char('s' | 'S') => WindowShortcut::Split(ui::WorkspaceSplitMode::TopBottom),
            InputKey::Char('o' | 'O') => WindowShortcut::Maximize,
            InputKey::Char('q' | 'Q') => WindowShortcut::Close,
            InputKey::Char('X') => WindowShortcut::Kill,
            InputKey::Char('x') if key.modifiers.shift => WindowShortcut::Kill,
            InputKey::Left | InputKey::Char('h' | 'H') => WindowShortcut::MoveFocus(-1, 0),
            InputKey::Right | InputKey::Char('l' | 'L') => WindowShortcut::MoveFocus(1, 0),
            InputKey::Up | InputKey::Char('k' | 'K') => WindowShortcut::MoveFocus(0, -1),
            InputKey::Down | InputKey::Char('j' | 'J') => WindowShortcut::MoveFocus(0, 1),
            InputKey::Escape => WindowShortcut::Consume,
            _ => WindowShortcut::Pass,
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/session/window_state.rs"]
mod tests;
