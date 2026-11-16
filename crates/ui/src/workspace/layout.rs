use ratatui::layout::{Margin, Rect};
use workspace_protocol::{MIN_CONTENT_SIZE, SessionId, Size};

use crate::{MIN_SHELL_TERMINAL_HEIGHT, MIN_SHELL_TERMINAL_WIDTH};

use super::{WorkspaceTree, chrome::WorkspaceWindowControls};

const PANE_FRAME: u16 = 2;
const PANE_GAP: usize = 1;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum WorkspaceSplitMode {
    #[default]
    LeftRight,
    TopBottom,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkspacePaneLayout {
    pub id: SessionId,
    pub area: Rect,
    pub title: Option<Rect>,
    pub content: Rect,
    pub controls: WorkspaceWindowControls,
}

impl WorkspacePaneLayout {
    /// Provides window controls and space for a resize notice while app rendering is blocked.
    pub fn for_blocked_window(id: SessionId, area: Rect) -> Option<Self> {
        (area.width >= WorkspaceWindowControls::MIN_WIDTH + PANE_FRAME && area.height > PANE_FRAME)
            .then(|| Self::new(id, area))
    }

    pub fn new(id: SessionId, area: Rect) -> Self {
        let content = area.inner(Margin::new(1, 1));
        let top = Rect::new(content.x, area.y, content.width, 1);
        let controls = WorkspaceWindowControls::new(top);
        let title = Rect::new(top.x, top.y, controls.minimize.x - top.x, 1);
        Self {
            id,
            area,
            title: Some(title),
            content,
            controls,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceLayout {
    pub available: Rect,
    pub capacity: usize,
    pub columns: usize,
    pub rows: usize,
    pub blocked: bool,
    pub size_needed: Size,
    pub panes: Vec<WorkspacePaneLayout>,
}

impl WorkspaceLayout {
    pub fn new(
        terminal: Rect,
        available: Rect,
        windows: &[(SessionId, Size)],
        visible: &[SessionId],
        focused: Option<SessionId>,
        tree: Option<&WorkspaceTree>,
    ) -> Self {
        let limits = Limits::new(terminal, available, windows);
        let mut result = Self {
            available,
            capacity: limits.capacity(),
            columns: limits.columns,
            rows: limits.rows,
            blocked: !limits.single_fits,
            size_needed: limits.size_needed,
            panes: Vec::new(),
        };
        if result.blocked {
            return result;
        }
        if let Some(tree) = tree {
            // At most four leaves: examine stable subsets while retaining the focused app.
            for count in (1..=visible.len()).rev() {
                let mut found = None;
                for mask in 1u32..(1u32 << visible.len()) {
                    if mask.count_ones() as usize != count {
                        continue;
                    }
                    let ids = visible
                        .iter()
                        .enumerate()
                        .filter_map(|(i, id)| (mask & (1 << i) != 0).then_some(*id))
                        .collect::<Vec<_>>();
                    if focused.is_some_and(|id| visible.contains(&id) && !ids.contains(&id)) {
                        continue;
                    }
                    if let Some(candidate) = tree.retain(&ids)
                        && result.fits(&candidate)
                    {
                        found = Some(candidate);
                        break;
                    }
                }
                if let Some(tree) = found {
                    place(&tree, available, &mut result.panes);
                    break;
                }
            }
        }
        result
    }

    pub fn capacity(terminal: Rect, available: Rect, windows: &[(SessionId, Size)]) -> usize {
        Limits::new(terminal, available, windows).capacity()
    }

    pub fn fits(&self, tree: &WorkspaceTree) -> bool {
        let (columns, rows) = tree.dimensions();
        !self.blocked && columns <= self.columns && rows <= self.rows && tree.ids().len() <= 4
    }

    pub fn can_split(&self, tree: &WorkspaceTree, id: SessionId, axis: WorkspaceSplitMode) -> bool {
        let mut next = tree.clone();
        self.panes.iter().any(|pane| pane.id == id)
            && next.split(id, SessionId(0), axis)
            && self.fits(&next)
    }
}

fn place(tree: &WorkspaceTree, area: Rect, panes: &mut Vec<WorkspacePaneLayout>) {
    match tree {
        WorkspaceTree::Window(id) => panes.push(WorkspacePaneLayout::new(*id, area)),
        WorkspaceTree::Split {
            axis,
            first,
            second,
        } => {
            let (a, b) = match axis {
                WorkspaceSplitMode::LeftRight => {
                    let (x, width) = split_span(area.x, area.width, 2, 0);
                    let (x2, width2) = split_span(area.x, area.width, 2, 1);
                    (
                        Rect::new(x, area.y, width, area.height),
                        Rect::new(x2, area.y, width2, area.height),
                    )
                }
                WorkspaceSplitMode::TopBottom => {
                    let (y, height) = split_span(area.y, area.height, 2, 0);
                    let (y2, height2) = split_span(area.y, area.height, 2, 1);
                    (
                        Rect::new(area.x, y, area.width, height),
                        Rect::new(area.x, y2, area.width, height2),
                    )
                }
            };
            place(first, a, panes);
            place(second, b, panes);
        }
    }
}

struct Limits {
    size_needed: Size,
    single_fits: bool,
    rows: usize,
    columns: usize,
}

impl Limits {
    fn new(terminal: Rect, available: Rect, windows: &[(SessionId, Size)]) -> Self {
        let minimum = windows
            .iter()
            .fold(MIN_CONTENT_SIZE, |minimum, (_, size)| Size {
                width: minimum.width.max(size.width),
                height: minimum.height.max(size.height),
            });
        let size_needed = Size {
            width: minimum
                .width
                .saturating_add(PANE_FRAME)
                .saturating_add(terminal.width.saturating_sub(available.width))
                .max(MIN_SHELL_TERMINAL_WIDTH),
            height: minimum
                .height
                .saturating_add(PANE_FRAME)
                .saturating_add(terminal.height.saturating_sub(available.height))
                .max(MIN_SHELL_TERMINAL_HEIGHT),
        };
        let frame_width = usize::from(minimum.width) + usize::from(PANE_FRAME);
        let frame_height = usize::from(minimum.height) + usize::from(PANE_FRAME);
        let rows = (usize::from(available.height) + PANE_GAP) / (frame_height + PANE_GAP);
        let columns = (usize::from(available.width) + PANE_GAP) / (frame_width + PANE_GAP);
        let (rows, columns) = (rows.min(2), columns.min(2));
        Self {
            size_needed,
            single_fits: terminal.width >= MIN_SHELL_TERMINAL_WIDTH
                && terminal.height >= MIN_SHELL_TERMINAL_HEIGHT
                && usize::from(available.width) >= frame_width
                && usize::from(available.height) >= frame_height,
            rows,
            columns,
        }
    }

    fn capacity(&self) -> usize {
        if self.single_fits {
            (self.rows * self.columns).max(1)
        } else {
            0
        }
    }
}

fn split_span(origin: u16, length: u16, count: usize, index: usize) -> (u16, u16) {
    let span = usize::from(length) - (count - 1) * PANE_GAP;
    let base = span / count;
    let remainder = span % count;
    let offset = index * (base + PANE_GAP) + index.min(remainder);
    let length = base + usize::from(index < remainder);
    (origin + offset as u16, length as u16)
}
