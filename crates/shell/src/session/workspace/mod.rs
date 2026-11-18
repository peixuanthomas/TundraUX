use super::window_state::{WindowPrefix, WindowSelection, WindowShortcut};
use super::*;
use crate::process_host::{WindowConnection, WorkerConnection};
use platform::management::ManagementKind;
use ratatui::Frame;
use ui::{
    WorkspaceAction as Action, WorkspaceCloseWait, WorkspaceLayout, WorkspacePaneLayout,
    WorkspacePanePlaceholder, WorkspacePanel, WorkspacePanelViewModel, WorkspaceSplitMenu,
    WorkspaceSplitMenuViewModel, WorkspaceSplitMode, WorkspaceTaskbar, WorkspaceTaskbarAction,
    WorkspaceTaskbarItem, WorkspaceWindowAction, WorkspaceWindowButtonsState,
    WorkspaceWindowViewModel,
};
use workspace_protocol::{
    AppStartupTimings, AppToShell, CursorShape, LayoutGeneration, NativePath, ProcessState,
    SessionId, ShellAction as HostAction, ShellToApp, Size, SupervisorToWorker, WindowFrame,
    WindowInit, WindowStatus, WorkerId, WorkerRole, WorkerToSupervisor,
};

mod chooser;
mod input;
mod performance;
mod render;
mod taskbar;
mod windows;

use performance::PerformanceTrace;

const CLOSE_WAIT: Duration = Duration::from_secs(2);
const STARTING_DELAY: Duration = Duration::from_millis(100);

struct Window {
    launch: WindowLaunch,
    worker: Option<WorkerId>,
    stream: Option<WindowConnection>,
    init: Option<WindowInit>,
    frame: Option<Arc<WindowFrame>>,
    presented: Option<Arc<WindowFrame>>,
    presenting: Option<Arc<WindowFrame>>,
    starting_presenting: bool,
    status: WindowStatus,
    state: ProcessState,
    queued: VecDeque<ShellToApp>,
    viewport: Option<ShellToApp>,
    close: CloseProgress,
    picked_file: Option<NativePath>,
    force_when_started: bool,
    view_requested: Option<ShellToApp>,
    launch_error: bool,
    input_blocked: bool,
    startup: Option<WindowStartup>,
}

struct WindowStartup {
    requested: Instant,
    accepted: Option<Instant>,
    starting_presented: bool,
    app_started: Option<Instant>,
    init_queued: Option<Instant>,
    process_spawn_us: u64,
    guardian_ready_wait_us: u64,
    app_timings: Option<AppStartupTimings>,
}

impl Window {
    fn initial_placeholder(&self, now: Instant) -> WorkspacePanePlaceholder {
        if self
            .startup
            .as_ref()
            .is_some_and(|startup| now.duration_since(startup.requested) > STARTING_DELAY)
        {
            WorkspacePanePlaceholder::Starting
        } else {
            WorkspacePanePlaceholder::Pending
        }
    }

    fn finish_cleanup(&mut self) {
        self.close = CloseProgress::Exiting;
    }

    fn update_status(&mut self, mut status: WindowStatus) {
        if matches!(
            self.close,
            CloseProgress::Cleaning(_) | CloseProgress::Waiting | CloseProgress::Exiting
        ) {
            // Reaping the entire process tree determines when its minimum is released.
            status.minimum = self.status.minimum;
        }
        self.status = status;
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum CloseProgress {
    #[default]
    Idle,
    Requested(Instant),
    Confirmation {
        size_notice_dismissed: bool,
    },
    Cleaning(Instant),
    Waiting,
    Exiting,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChromeAction {
    Back,
    Help,
    Tasks,
    Keyboard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PointerTarget {
    Pane(SessionId),
    PaneFrame(SessionId),
    WindowButton(SessionId, WorkspaceWindowAction),
    Taskbar(WorkspaceTaskbarAction),
    Overlay(Action),
    Chrome(ChromeAction),
}

#[derive(Debug, Clone, Copy)]
struct PointerCapture {
    target: PointerTarget,
    generation: LayoutGeneration,
    button: PointerButton,
}

pub(super) struct Workspace {
    performance: PerformanceTrace,
    image_cell_size: Option<[u16; 2]>,
    text_sizing: bool,
    windows: BTreeMap<SessionId, Window>,
    selection: WindowSelection,
    chooser: Option<chooser::WindowChooser>,
    split_launch: Option<(SessionId, SessionId)>,
    generation: LayoutGeneration,
    layout: WorkspaceLayout,
    width: u16,
    height: u16,
    input_focus: Option<SessionId>,
    terminal_focused: bool,
    last_pointer: Option<CellPosition>,
    pointer_hovered: bool,
    capture: Option<PointerCapture>,
    suppress_release: Option<PointerButton>,
    prefix: WindowPrefix,
    panel: Option<WorkspacePanelViewModel>,
    panel_hits: Vec<(Action, Rect)>,
    panel_scroll_rows: usize,
    panel_max_scroll: usize,
    wait_for: Option<SessionId>,
    modal_focus: Option<Action>,
    panel_key_focus: bool,
    split_window: Option<SessionId>,
    split_model: WorkspaceSplitMenuViewModel,
    split_menu: Option<WorkspaceSplitMenu>,
    taskbar: Option<WorkspaceTaskbar>,
    taskbar_scroll: usize,
    taskbar_focus: Option<SessionId>,
    taskbar_order: Vec<SessionId>,
    taskbar_menu: Option<taskbar::TaskbarMenu>,
    deleting_launcher: HashSet<SessionId>,
    chrome_layout: Option<ui::ShellFrameLayout>,
    toast_area: Option<Rect>,
    pending_global: Option<ShellCommand>,
    pending_update: Option<PathBuf>,
    input_serial: u64,
    input_recipients: HashSet<SessionId>,
    input_fences: BTreeMap<SessionId, u64>,
    input_rejected: bool,
    input_busy_notice: bool,
    task_catalogs: BTreeMap<SessionId, Vec<String>>,
    tasks_return: Option<(SessionId, SessionId)>,
    toggle_tasks_requested: bool,
    shutdown: bool,
    reset: bool,
    frozen: bool,
    placeholder_redraw_at: Option<Instant>,
}

impl Workspace {
    pub(super) fn new(image_cell_size: Option<[u16; 2]>, text_sizing: bool) -> Self {
        Self {
            performance: PerformanceTrace::new(),
            image_cell_size,
            text_sizing,
            windows: BTreeMap::new(),
            selection: Default::default(),
            chooser: None,
            split_launch: None,
            generation: LayoutGeneration(0),
            layout: WorkspaceLayout::new(Rect::default(), Rect::default(), &[], &[], None, None),
            width: 0,
            height: 0,
            input_focus: None,
            terminal_focused: true,
            last_pointer: None,
            pointer_hovered: false,
            capture: None,
            suppress_release: None,
            prefix: WindowPrefix::default(),
            panel: None,
            panel_hits: Vec::new(),
            panel_scroll_rows: 0,
            panel_max_scroll: 0,
            wait_for: None,
            modal_focus: None,
            panel_key_focus: true,
            split_window: None,
            split_model: Default::default(),
            split_menu: None,
            taskbar: None,
            taskbar_scroll: 0,
            taskbar_focus: None,
            taskbar_order: Vec::new(),
            taskbar_menu: None,
            deleting_launcher: HashSet::new(),
            chrome_layout: None,
            toast_area: None,
            pending_global: None,
            pending_update: None,
            input_serial: 0,
            input_recipients: HashSet::new(),
            input_fences: BTreeMap::new(),
            input_rejected: false,
            input_busy_notice: false,
            task_catalogs: BTreeMap::new(),
            tasks_return: None,
            toggle_tasks_requested: false,
            shutdown: false,
            reset: false,
            frozen: false,
            placeholder_redraw_at: None,
        }
    }

    pub(super) fn active(&self) -> bool {
        !self.windows.is_empty()
    }

    pub(super) fn text_sizing(&self) -> bool {
        self.text_sizing
    }

    pub(super) fn shutdown_requested(&self) -> bool {
        self.shutdown
    }

    pub(super) fn reset_requested(&self) -> bool {
        self.reset && !self.active()
    }

    pub(super) fn take_update(&mut self) -> Option<PathBuf> {
        (!self.active())
            .then(|| self.pending_update.take())
            .flatten()
    }

    pub(super) fn poll_timeout(&self) -> Duration {
        let now = Instant::now();
        let maximum = self
            .windows
            .values()
            .filter(|_| self.wait_for.is_none())
            .filter_map(|window| match window.close {
                CloseProgress::Requested(at) | CloseProgress::Cleaning(at) => {
                    Some((at + CLOSE_WAIT).saturating_duration_since(now))
                }
                _ => None,
            })
            .min()
            .unwrap_or(watchdog::WORKER_PROGRESS_INTERVAL)
            .min(watchdog::WORKER_PROGRESS_INTERVAL);
        self.placeholder_redraw_at.map_or(maximum, |deadline| {
            maximum.min(deadline.saturating_duration_since(now))
        })
    }

    fn placeholder_redraw_due(&mut self, now: Instant) -> bool {
        if self
            .placeholder_redraw_at
            .is_some_and(|deadline| now >= deadline)
        {
            self.placeholder_redraw_at = None;
            true
        } else {
            false
        }
    }

    pub(super) fn did_present(&mut self, state: &ShellSession, terminal_output_us: u64) {
        for (&id, window) in &mut self.windows {
            if std::mem::take(&mut window.starting_presenting)
                && let Some(startup) = &mut window.startup
            {
                startup.starting_presented = true;
            }
            if let Some(frame) = window.presenting.take() {
                self.performance.did_present(
                    id,
                    frame.sequence,
                    frame.processed_input_serial,
                    terminal_output_us,
                );
                window.presented = Some(frame);
                if let Some(startup) = window.startup.take() {
                    let accepted = startup.accepted;
                    let starting_shown = startup.starting_presented;
                    startup.record_presented(
                        id,
                        window.launch.id(),
                        window.worker,
                        state.auth_session(),
                        terminal_output_us,
                        starting_shown,
                        accepted,
                    );
                }
            }
        }
    }

    pub(super) fn hides_home(&self) -> bool {
        self.layout.blocked || !self.layout.panes.is_empty()
    }

    pub(super) fn blocked(&self) -> bool {
        self.layout.blocked
    }

    pub(super) fn remember_chrome(
        &mut self,
        layout: ui::ShellFrameLayout,
        toast_area: Option<Rect>,
    ) {
        self.chrome_layout = Some(layout);
        self.toast_area = toast_area;
    }

    pub(super) fn invalidate_layout(&mut self) {
        self.cancel_capture();
        self.pointer_hovered = false;
        self.chrome_layout = None;
        self.toast_area = None;
        self.taskbar = None;
        self.close_split_menu();
        self.taskbar_menu = None;
        self.input_focus = None;
        self.layout.panes.clear();
        self.generation.0 += 1;
    }

    pub(super) fn has_native_input(&self) -> bool {
        self.input_focus
            .and_then(|id| self.windows.get(&id))
            .and_then(|window| window.frame.as_ref())
            .is_some_and(|frame| !frame.modal && !frame.native_escape.is_empty())
    }

    pub(super) fn finish_input_batch(&mut self, external: bool) {
        if !external {
            self.input_recipients.clear();
            return;
        }
        self.input_serial += 1;
        for id in std::mem::take(&mut self.input_recipients) {
            if self.enqueue(
                id,
                ShellToApp::InputFence {
                    serial: self.input_serial,
                },
            ) {
                self.input_fences.insert(id, self.input_serial);
            }
        }
    }

    pub(super) fn external_snapshot_ready(&self) -> bool {
        self.input_fences.is_empty()
            && self
                .layout
                .panes
                .iter()
                .filter_map(|pane| self.windows.get(&pane.id))
                .all(|window| {
                    window
                        .frame
                        .as_ref()
                        .is_some_and(|frame| frame.layout == self.generation)
                        || window.state == ProcessState::Unresponsive
                        || matches!(window.state, ProcessState::ForceStopping)
                        || matches!(
                            window.close,
                            CloseProgress::Cleaning(_)
                                | CloseProgress::Waiting
                                | CloseProgress::Exiting
                        )
                        || window.status.unresponsive
                })
    }

    pub(super) fn take_input_rejection(&mut self) -> (bool, bool) {
        (
            std::mem::take(&mut self.input_rejected),
            std::mem::take(&mut self.input_busy_notice),
        )
    }

    pub(super) fn focused_page(&self) -> Option<&str> {
        if self.layout.blocked {
            return Some("terminal_size");
        }
        if self.layout.panes.is_empty() {
            return None;
        }
        let window = self.windows.get(&self.selection.focused()?)?;
        window.frame.as_ref().map(|frame| frame.page.as_str())
    }

    pub(super) fn focused_status(&self) -> Option<&str> {
        if self.layout.blocked || self.layout.panes.is_empty() {
            return None;
        }
        self.windows
            .get(&self.selection.focused()?)?
            .frame
            .as_ref()
            .map(|frame| frame.status.as_str())
            .filter(|status| !status.is_empty())
    }

    pub(super) fn keyboard_available(&self) -> bool {
        !self.layout.blocked
            && !self.layout.panes.is_empty()
            && self
                .selection
                .focused()
                .and_then(|id| self.windows[&id].frame.as_ref())
                .is_some_and(|frame| frame.size.width > 0 && frame.keyboard_available)
    }

    pub(super) fn busy(&self) -> bool {
        self.windows.values().any(|window| window.status.busy)
    }

    pub(super) fn tasks_label(&self) -> Option<String> {
        if self
            .windows
            .values()
            .any(|window| window.status.awaiting_approval)
        {
            Some(i18n::tr!("ui-shell-tasks-waiting"))
        } else if self
            .windows
            .values()
            .any(|window| window.status.running_operations)
        {
            Some(i18n::tr!("ui-shell-tasks-active"))
        } else {
            None
        }
    }

    pub(super) fn animating(&self) -> bool {
        self.placeholder_redraw_at.is_some()
            || self
                .layout
                .panes
                .iter()
                .filter_map(|pane| self.windows.get(&pane.id))
                .any(|window| {
                    window.frame.as_ref().is_some_and(|frame| {
                        frame.layout == self.generation && frame.animation_running
                    })
                })
    }

    pub(super) fn cursor_shape(&self) -> Option<CursorShape> {
        if self.has_overlay() {
            return None;
        }
        let window = self.windows.get(&self.input_focus?)?;
        if matches!(window.state, ProcessState::Unresponsive)
            || window.status.unresponsive
            || matches!(
                window.close,
                CloseProgress::Cleaning(_) | CloseProgress::Waiting | CloseProgress::Exiting
            )
        {
            return None;
        }
        let frame = window.frame.as_ref()?;
        (frame.layout == self.generation).then_some(frame.cursor?.shape)
    }

    pub(super) fn relayout(&mut self, bounds: Rect, available: Rect, modal: bool) -> bool {
        let minimums = self
            .windows
            .iter()
            .map(|(&id, window)| (id, window.status.minimum))
            .collect::<Vec<_>>();
        let focused = self.selection.focused();
        let visible = self.selection.visible();
        let mut layout = WorkspaceLayout::new(
            bounds,
            available,
            &minimums,
            &visible,
            focused,
            self.selection.tree.as_ref(),
        );
        if self.chooser.is_some() && !layout.panes.iter().any(|pane| pane.id == SessionId(0)) {
            self.cancel_chooser();
            layout = WorkspaceLayout::new(
                bounds,
                available,
                &minimums,
                &self.selection.visible(),
                self.selection.focused(),
                self.selection.tree.as_ref(),
            );
        }
        let dimensions_changed = self.width != bounds.width
            || self.height != bounds.height
            || self.layout.available != available;
        if dimensions_changed {
            self.prefix.clear();
            if let Some(chooser) = &mut self.chooser {
                chooser.list.cancel_pointer();
            }
        }
        if modal || dimensions_changed {
            self.close_split_menu();
            self.taskbar_menu = None;
        }
        let mut overlay_changed = false;
        if let Some(id) = self.wait_for
            && !layout.blocked
            && self
                .windows
                .get(&id)
                .is_some_and(|window| matches!(window.close, CloseProgress::Confirmation { .. }))
        {
            self.hide_close_wait(id);
            overlay_changed = true;
        }
        if let Some(id) = focused
            && layout.blocked
            && self.wait_for.is_none()
            && self.windows.get(&id).is_some_and(|window| {
                matches!(
                    window.close,
                    CloseProgress::Confirmation {
                        size_notice_dismissed: false
                    }
                )
            })
        {
            self.show_close_wait(id);
            overlay_changed = true;
        }
        let input_focus = focused.filter(|id| {
            !modal
                && self.terminal_focused
                && !self.has_overlay()
                && layout.panes.iter().any(|pane| pane.id == *id)
        });
        let frozen = layout.blocked || self.frozen;
        let changed = self.layout != layout
            || self.input_focus != input_focus
            || self.width != bounds.width
            || self.height != bounds.height;
        self.width = bounds.width;
        self.height = bounds.height;
        if dimensions_changed {
            self.pointer_hovered = false;
        }
        if changed {
            self.cancel_capture();
            self.generation.0 += 1;
            self.layout = layout;
            self.input_focus = input_focus;
        }
        self.refresh_chooser_area();
        let mut updated = false;
        for (&id, window) in &mut self.windows {
            let pane = self.layout.panes.iter().find(|pane| pane.id == id);
            let size = (!frozen).then(|| {
                pane.map_or(window.status.minimum, |pane| {
                    Size::new(pane.content.width, pane.content.height)
                })
            });
            let message = ShellToApp::Viewport {
                layout: self.generation,
                size,
                visible: pane.is_some(),
                focused: self.input_focus == Some(id),
            };
            if window.viewport.as_ref() != Some(&message) {
                window.viewport = Some(message.clone());
                if window.init.is_none() {
                    window.view_requested = Some(message);
                }
                updated = true;
            }
        }
        changed || updated || overlay_changed
    }

    fn blocked_pane(&self) -> Option<WorkspacePaneLayout> {
        if !self.layout.blocked {
            return None;
        }
        WorkspacePaneLayout::for_blocked_window(self.selection.focused()?, self.layout.available)
    }

    fn close_split_menu(&mut self) {
        if self.split_window.take().is_some() {
            self.split_menu = None;
            self.cancel_capture();
        }
    }

    fn show_close_wait(&mut self, id: SessionId) {
        self.close_split_menu();
        self.taskbar_menu = None;
        self.cancel_capture();
        self.wait_for = Some(id);
        self.modal_focus = Some(Action::Dismiss);
    }

    fn hide_close_wait(&mut self, id: SessionId) {
        if self.wait_for == Some(id) {
            self.wait_for = None;
            self.modal_focus = None;
            self.panel_hits.clear();
            self.cancel_capture();
        }
    }

    pub(super) fn set_suspend(&mut self, suspended: bool) {
        self.frozen = suspended;
        if let Some(layout) = self.chrome_layout {
            self.relayout(layout.bounds, layout.main, suspended);
        }
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/session/workspace_close.rs"]
mod close_tests;

#[cfg(test)]
#[path = "../../../tests/unit/session/workspace_presentation.rs"]
mod presentation_tests;
