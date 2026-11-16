use workspace_protocol::SessionId;

use super::WorkspaceSplitMode;

/// The requested topology survives size-driven hiding; each axis occurs once per path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceTree {
    Window(SessionId),
    Split {
        axis: WorkspaceSplitMode,
        first: Box<Self>,
        second: Box<Self>,
    },
}

impl WorkspaceTree {
    pub fn ids(&self) -> Vec<SessionId> {
        match self {
            Self::Window(id) => vec![*id],
            Self::Split { first, second, .. } => {
                let mut ids = first.ids();
                ids.extend(second.ids());
                ids
            }
        }
    }

    pub fn dimensions(&self) -> (usize, usize) {
        match self {
            Self::Window(_) => (1, 1),
            Self::Split {
                axis,
                first,
                second,
            } => {
                let (a, b) = first.dimensions();
                let (c, d) = second.dimensions();
                match axis {
                    WorkspaceSplitMode::LeftRight => (a + c, b.max(d)),
                    WorkspaceSplitMode::TopBottom => (a.max(c), b + d),
                }
            }
        }
    }

    pub fn split(&mut self, id: SessionId, next: SessionId, axis: WorkspaceSplitMode) -> bool {
        if self.ids().len() >= 4 || self.ids().contains(&next) {
            return false;
        }
        self.split_at(id, next, axis, false)
    }

    fn split_at(
        &mut self,
        id: SessionId,
        next: SessionId,
        axis: WorkspaceSplitMode,
        used: bool,
    ) -> bool {
        match self {
            Self::Window(current) if *current == id && !used => {
                *self = Self::Split {
                    axis,
                    first: Box::new(Self::Window(id)),
                    second: Box::new(Self::Window(next)),
                };
                true
            }
            Self::Split {
                axis: parent,
                first,
                second,
            } => {
                let used = used || *parent == axis;
                first.split_at(id, next, axis, used) || second.split_at(id, next, axis, used)
            }
            _ => false,
        }
    }

    pub fn replace(&mut self, id: SessionId, next: SessionId) {
        match self {
            Self::Window(current) if *current == id => *current = next,
            Self::Split { first, second, .. } => {
                first.replace(id, next);
                second.replace(id, next);
            }
            _ => {}
        }
    }

    pub fn retain(&self, ids: &[SessionId]) -> Option<Self> {
        match self {
            Self::Window(id) => ids.contains(id).then(|| self.clone()),
            Self::Split {
                axis,
                first,
                second,
            } => match (first.retain(ids), second.retain(ids)) {
                (Some(first), Some(second)) => Some(Self::Split {
                    axis: *axis,
                    first: Box::new(first),
                    second: Box::new(second),
                }),
                (first, second) => first.or(second),
            },
        }
    }

    pub fn without(&self, id: SessionId) -> Option<Self> {
        self.retain(
            &self
                .ids()
                .into_iter()
                .filter(|other| *other != id)
                .collect::<Vec<_>>(),
        )
    }

    pub fn restore(&self, previous: &Self, id: SessionId) -> Option<Self> {
        let (axis, before, sibling) = previous.placement(id)?;
        let mut result = self.clone();
        if result.insert_beside(&sibling, id, axis, before) {
            let (columns, rows) = result.dimensions();
            if columns <= 2 && rows <= 2 && result.ids().len() <= 4 {
                return Some(result);
            }
        }
        None
    }

    fn placement(&self, id: SessionId) -> Option<(WorkspaceSplitMode, bool, Self)> {
        let Self::Split {
            axis,
            first,
            second,
        } = self
        else {
            return None;
        };
        if **first == Self::Window(id) {
            Some((*axis, true, *second.clone()))
        } else if **second == Self::Window(id) {
            Some((*axis, false, *first.clone()))
        } else {
            first.placement(id).or_else(|| second.placement(id))
        }
    }

    fn insert_beside(
        &mut self,
        sibling: &Self,
        id: SessionId,
        axis: WorkspaceSplitMode,
        before: bool,
    ) -> bool {
        if self == sibling {
            let (first, second) = if before {
                (Self::Window(id), self.clone())
            } else {
                (self.clone(), Self::Window(id))
            };
            *self = Self::Split {
                axis,
                first: Box::new(first),
                second: Box::new(second),
            };
            true
        } else if let Self::Split { first, second, .. } = self {
            first.insert_beside(sibling, id, axis, before)
                || second.insert_beside(sibling, id, axis, before)
        } else {
            false
        }
    }
}
