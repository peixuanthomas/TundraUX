use super::*;
use crate::process_host::transport::{
    MAX_CONTROL_BYTES, MAX_MESSAGES, Message, RESERVED_BYTES, RESERVED_MESSAGES,
};
use std::sync::atomic::{AtomicU64, Ordering};

impl Workspace {
    pub(in crate::session) fn poll(&mut self, state: &mut ShellSession) -> io::Result<bool> {
        let mut redraw = self.placeholder_redraw_due(Instant::now());
        let connection = WorkerConnection::current().expect("managed workspace");
        connection.progress()?;
        while let Some(message) = connection.try_recv()? {
            match message {
                SupervisorToWorker::AppStarted {
                    worker,
                    pid,
                    endpoint,
                    process_spawn_us,
                    guardian_ready_wait_us,
                } => {
                    let WorkerRole::App(id) = worker.role else {
                        continue;
                    };
                    if let Some(window) = self.windows.get_mut(&id)
                        && window.worker.is_none_or(|current| current == worker)
                    {
                        window.worker = Some(worker);
                        if let Some(startup) = &mut window.startup {
                            startup.app_started = Some(Instant::now());
                            startup.process_spawn_us = process_spawn_us;
                            startup.guardian_ready_wait_us = guardian_ready_wait_us;
                        }
                        match connection.connect_app(worker, pid, &endpoint) {
                            Ok(stream) => window.stream = Some(stream),
                            Err(error) => {
                                report_launch_failure(state, error.to_string());
                                self.force(id)?;
                            }
                        }
                    }
                    redraw = true;
                }
                SupervisorToWorker::AppState {
                    worker,
                    state: status,
                } => {
                    let WorkerRole::App(id) = worker.role else {
                        continue;
                    };
                    if let Some(window) = self.windows.get_mut(&id)
                        && window.worker.is_none_or(|current| current == worker)
                    {
                        window.worker = Some(worker);
                        window.state = status;
                        if matches!(
                            status,
                            ProcessState::Unresponsive | ProcessState::ForceStopping
                        ) {
                            self.input_fences.remove(&id);
                        }
                        if let ProcessState::Exited { code, forced } = status {
                            let launching = window.frame.is_none()
                                && matches!(window.close, CloseProgress::Idle)
                                && !window.force_when_started
                                && !window.launch_error;
                            self.finish_window_exit(id, code, forced, state);
                            if launching {
                                report_launch_failure(
                                    state,
                                    i18n::tr!("ui-components-unavailable"),
                                );
                            }
                        } else if window.force_when_started {
                            window.force_when_started = false;
                            self.force(id)?;
                        }
                    }
                    redraw = true;
                }
                SupervisorToWorker::Close | SupervisorToWorker::Exit => {
                    self.shutdown = true;
                }
                SupervisorToWorker::CleanupError { worker, error } => {
                    if let WorkerRole::App(id) = worker.role
                        && let Some(window) = self.windows.get(&id)
                        && window.worker == Some(worker)
                    {
                        state.notify_alert_with_tone(
                            i18n::msg!(
                                "workspace-stop-failed",
                                window = window.launch.title(),
                                reason = error
                            ),
                            ui::NotificationTone::Error,
                        );
                        redraw = true;
                    }
                }
                _ => {}
            }
        }

        let ids = self.windows.keys().copied().collect::<Vec<_>>();
        let mut received = Vec::new();
        for id in ids {
            let window = self.windows.get_mut(&id).expect("live window");
            flush_window_requests(window);
            let Some(stream) = &mut window.stream else {
                continue;
            };
            if window.input_blocked
                && !stream.write_backpressured()
                && window.queued.len() < MAX_MESSAGES / 2
                && window
                    .queued
                    .iter()
                    .map(Message::resident_bytes)
                    .sum::<usize>()
                    < MAX_CONTROL_BYTES / 2
            {
                window.input_blocked = false;
                redraw = true;
            }
            match stream.poll() {
                Ok(messages) => received.extend(messages.into_iter().map(|message| (id, message))),
                Err(_) => {
                    window.stream = None;
                    window.queued.clear();
                    self.input_fences.remove(&id);
                    if !matches!(
                        window.close,
                        CloseProgress::Cleaning(_)
                            | CloseProgress::Waiting
                            | CloseProgress::Exiting
                    ) {
                        window.state = ProcessState::Unresponsive;
                    }
                    redraw = true;
                }
            }
        }
        for (id, message) in received {
            redraw |= self.receive(id, message, state)?;
        }
        self.sync_task_catalogs();
        // A modal prompt is user time. Start the timer when work starts, or
        // while a close request has yet to get any response from the app.
        if self.wait_for.is_none()
            && let Some((&id, _)) = self.windows.iter().find(|(_, window)| {
                matches!(
                    window.close,
                    CloseProgress::Requested(at) | CloseProgress::Cleaning(at)
                        if at.elapsed() >= CLOSE_WAIT
                )
            })
        {
            self.show_close_wait(id);
            redraw = true;
        }

        if let Some(command) = state.window_global_request.take() {
            self.pending_global = Some(command);
        }
        if self.pending_global.is_some() || self.pending_update.is_some() || self.reset {
            if let Some((&id, window)) = self.windows.iter().next() {
                if window.close == CloseProgress::Idle && !window.force_when_started {
                    self.request_close(id);
                    redraw = true;
                }
            } else if let Some(command) = self.pending_global.take() {
                state.windows_active = false;
                let platform = platform::native_platform();
                let action = state.apply_routed_event(
                    RoutedEvent {
                        input: InputEvent::Tick,
                        target: RoutedTarget::Global,
                        command,
                    },
                    platform.as_ref(),
                    Instant::now(),
                );
                match action {
                    ShellAction::PowerOff | ShellAction::Reboot => {
                        state.start_auto_admin_power(
                            action == ShellAction::Reboot,
                            Arc::from(platform),
                        );
                    }
                    ShellAction::Exit => self.shutdown = true,
                    ShellAction::Redraw => {}
                }
                redraw = true;
            }
        }
        state.windows_active = self.active();
        Ok(redraw)
    }

    pub(in crate::session) fn launch_pending(
        &mut self,
        state: &mut ShellSession,
    ) -> io::Result<bool> {
        if crate::session_services::SessionServicesClient::current()
            .is_some_and(|services| !services.is_session_ready(state))
        {
            return Ok(false);
        }
        let mut launches = std::mem::take(&mut state.window_launches);
        let toggle_tasks = std::mem::take(&mut state.window_tasks_requested)
            | std::mem::take(&mut self.toggle_tasks_requested);
        let delete = state.window_delete_launcher.take();
        let redraw = !launches.is_empty() || toggle_tasks || delete.is_some();
        if let Some(id) = delete
            && let Some(entry) = self
                .windows
                .get(&id)
                .filter(|window| window.close == CloseProgress::Idle)
                .and_then(|window| window.launch.launcher_id())
            && state.can_manage_launcher()
            && state.current_launcher_entry(entry).is_some()
        {
            self.deleting_launcher.insert(id);
            self.request_close(id);
        }
        if toggle_tasks && self.pending_global.is_none() {
            self.toggle_tasks(state)?;
        }
        for request in launches.drain(..) {
            if self.pending_global.is_some() {
                break;
            }
            if let WindowRequest::Open {
                launch,
                new_session,
            } = request
            {
                self.open_requested(launch, new_session, state.auth_session())?;
            }
        }
        if self
            .chooser
            .as_ref()
            .is_some_and(|chooser| chooser.awaiting_launcher)
            && state.interactive_overlays().is_empty()
            && state.window_launches.is_empty()
        {
            self.cancel_chooser();
        }
        state.windows_active = self.active();
        Ok(redraw)
    }

    pub(super) fn open_requested(
        &mut self,
        launch: WindowLaunch,
        new_session: bool,
        auth: Option<&AuthSession>,
    ) -> io::Result<()> {
        if !new_session && self.restore_running_window(|running| launch.can_reuse(running)) {
            return Ok(());
        }
        self.open(launch, auth)?;
        Ok(())
    }

    pub(super) fn restore_running_window(
        &mut self,
        matches_launch: impl Fn(&WindowLaunch) -> bool,
    ) -> bool {
        let matches = |id: SessionId| {
            let window = &self.windows[&id];
            !matches!(
                window.close,
                CloseProgress::Cleaning(_) | CloseProgress::Waiting | CloseProgress::Exiting
            ) && !matches!(
                window.state,
                ProcessState::ForceStopping | ProcessState::Exited { .. }
            ) && matches_launch(&window.launch)
        };
        // Prefer the focused instance, then the newest live session for this app.
        let id = self
            .selection
            .focused()
            .filter(|id| matches(*id))
            .or_else(|| self.windows.keys().rev().copied().find(|&id| matches(id)));
        let Some(id) = id else {
            return false;
        };
        self.cancel_capture();
        self.selection.restore(id, &self.layout);
        true
    }

    pub(super) fn open(
        &mut self,
        launch: WindowLaunch,
        auth: Option<&AuthSession>,
    ) -> io::Result<SessionId> {
        // The watchdog can receive an older window ID after logout and login.
        static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);
        let id = SessionId(NEXT_SESSION.fetch_add(1, Ordering::Relaxed));
        let init = WindowInit {
            launch: serde_json::to_value(&launch)?,
            profile: auth.map(serde_json::to_value).transpose()?,
            language: None,
            size: workspace_protocol::MIN_CONTENT_SIZE,
            layout: self.generation,
            focused: false,
            image_cell_size: self.image_cell_size,
            text_sizing: self.text_sizing,
        };
        let requested = Instant::now();
        WorkerConnection::current()
            .expect("managed workspace")
            .send(WorkerToSupervisor::SpawnApp {
                session: id,
                application: launch.id().into(),
            })?;
        self.cancel_capture();
        if let Some(chooser) = self.chooser.take() {
            self.split_launch = Some((chooser.source, id));
        }
        self.selection.open(id);
        self.taskbar_order.push(id);
        self.windows.insert(
            id,
            Window {
                launch,
                worker: None,
                stream: None,
                init: Some(init),
                frame: None,
                presented: None,
                presenting: None,
                starting_presenting: false,
                status: WindowStatus::default(),
                state: ProcessState::Starting,
                queued: VecDeque::new(),
                viewport: None,
                close: CloseProgress::Idle,
                force_when_started: false,
                view_requested: None,
                launch_error: false,
                input_blocked: false,
                startup: Some(WindowStartup {
                    requested,
                    accepted: None,
                    starting_presented: false,
                    app_started: None,
                    init_queued: None,
                    process_spawn_us: 0,
                    guardian_ready_wait_us: 0,
                    app_timings: None,
                }),
            },
        );
        Ok(id)
    }

    pub(super) fn remove(&mut self, id: SessionId) {
        if self
            .chooser
            .as_ref()
            .is_some_and(|chooser| chooser.source == id)
        {
            self.cancel_chooser();
        }
        if let Some(chooser) = &mut self.chooser {
            chooser
                .list
                .commands
                .retain(|command| command.id.as_str() != format!("window.{id}"));
        }
        let focused = self.selection.focused() == Some(id);
        self.input_fences.remove(&id);
        self.input_recipients.remove(&id);
        self.task_catalogs.remove(&id);
        self.deleting_launcher.remove(&id);
        self.windows.remove(&id);
        self.performance.forget_window(id);
        self.taskbar_order.retain(|&window| window != id);
        if self
            .taskbar_menu
            .as_ref()
            .is_some_and(|menu| menu.window == id)
        {
            self.taskbar_menu = None;
        }
        self.layout.panes.retain(|pane| pane.id != id);
        if let Some(taskbar) = &mut self.taskbar {
            taskbar
                .hits
                .retain(|(action, _)| {
                    !matches!(action, WorkspaceTaskbarAction::Activate(window) | WorkspaceTaskbarAction::Close(window) if *window == id)
                });
        }
        self.panel_hits.retain(|(action, _)| {
            !matches!(
                action,
                Action::Select(window)
                    | Action::Switch(window)
                    | Action::Show(window)
                    | Action::Background(window)
                    | Action::Maximize(window)
                    | Action::Close(window)
                    | Action::ForceKill(window)
                    if *window == id
            )
        });
        if let Some(menu) = &mut self.split_menu {
            menu.hits.retain(|(action, _)| *action != Action::Show(id));
        }
        self.selection.remove(id);
        if let Some((source, launch)) = self.split_launch
            && launch == id
        {
            self.selection.focus(source);
            self.split_launch = None;
        }
        if self.split_window == Some(id) {
            self.close_split_menu();
        }
        if let Some((task, previous)) = self.tasks_return
            && task == id
        {
            self.tasks_return = None;
            if focused && self.windows.contains_key(&previous) {
                self.selection.switch(previous);
            }
        }
        self.hide_close_wait(id);
        if let Some(panel) = &mut self.panel
            && panel.selected == Some(id)
        {
            panel.selected = self
                .selection
                .focused()
                .or(self.windows.keys().next().copied());
        }
        self.cancel_capture();
    }

    pub(super) fn finish_window_exit(
        &mut self,
        id: SessionId,
        code: Option<i32>,
        forced: bool,
        state: &mut ShellSession,
    ) {
        if self.deleting_launcher.remove(&id)
            && code == Some(0)
            && !forced
            && let Some(entry) = self
                .windows
                .get(&id)
                .and_then(|window| window.launch.launcher_id())
        {
            state.remove_taskbar_launcher_entry(entry.to_owned());
        }
        self.remove(id);
    }

    fn toggle_tasks(&mut self, state: &ShellSession) -> io::Result<()> {
        let focused = self.selection.focused();
        let is_tasks = |window: &Window| {
            window.launch.application()
                == super::window_launch::WindowApplication::Management(ManagementKind::Tasks)
        };
        if let Some(id) = focused.filter(|id| is_tasks(&self.windows[id])) {
            self.request_close(id);
        } else if let Some((&id, _)) = self
            .windows
            .iter()
            .rev()
            .find(|(_, window)| is_tasks(window))
        {
            self.selection.switch(id);
            self.tasks_return = focused.map(|previous| (id, previous));
        } else {
            let id = self.open(
                WindowLaunch::Management(state.tasks_query()),
                state.auth_session(),
            )?;
            self.tasks_return = focused.map(|previous| (id, previous));
        }
        Ok(())
    }

    pub(super) fn receive(
        &mut self,
        id: SessionId,
        message: AppToShell,
        state: &mut ShellSession,
    ) -> io::Result<bool> {
        let Some(window) = self.windows.get_mut(&id) else {
            return Ok(false);
        };
        match message {
            AppToShell::Welcome { worker } if window.worker == Some(worker) => {
                if let Some(mut init) = window.init.take() {
                    if let Some(viewport) = &window.viewport {
                        init.update_viewport(viewport);
                    }
                    init.language = Some(state.export_window_language()?);
                    window.queued.push_front(ShellToApp::Init(init));
                    if let Some(viewport) = window.viewport.clone() {
                        window.queued.insert(1, viewport);
                    }
                    flush_window_requests(window);
                }
            }
            AppToShell::Welcome { .. } => self.force(id)?,
            AppToShell::Frame(mut frame) => {
                let timings = frame.startup_timings.take();
                if let Some(startup) = &mut window.startup
                    && let Some(timings) = timings
                {
                    startup.app_timings = Some(timings);
                }
                if frame.sequence != 0
                    && window
                        .frame
                        .as_ref()
                        .is_some_and(|previous| frame.sequence <= previous.sequence)
                {
                    return Ok(false);
                }
                if frame.layout != self.generation
                    || !self.layout.panes.iter().any(|pane| {
                        pane.id == id
                            && frame.size == Size::new(pane.content.width, pane.content.height)
                    })
                {
                    return Ok(false);
                }
                if self
                    .input_fences
                    .get(&id)
                    .is_some_and(|serial| frame.confirmed_input_fence >= *serial)
                {
                    self.input_fences.remove(&id);
                }
                if window.frame.as_deref() == Some(&frame) {
                    return Ok(false);
                }
                let modal_changed = window
                    .frame
                    .as_ref()
                    .is_some_and(|old| old.modal != frame.modal);
                self.performance.frame_accepted(id, frame.sequence);
                window.frame = Some(Arc::new(frame));
                if let Some(startup) = &mut window.startup
                    && startup.accepted.is_none()
                {
                    startup.accepted = Some(Instant::now());
                    startup.record(id, window.launch.id(), window.worker, state.auth_session());
                }
                if self.split_launch.is_some_and(|(_, launch)| launch == id) {
                    self.split_launch = None;
                }
                if modal_changed
                    && self.capture.is_some_and(|capture| match capture.target {
                        PointerTarget::Pane(target)
                        | PointerTarget::PaneFrame(target)
                        | PointerTarget::WindowButton(target, _) => target == id,
                        _ => false,
                    })
                {
                    self.cancel_capture();
                }
            }
            AppToShell::InputFence { serial } => {
                if !self.layout.panes.iter().any(|pane| pane.id == id)
                    && self
                        .input_fences
                        .get(&id)
                        .is_some_and(|latest| serial >= *latest)
                {
                    self.input_fences.remove(&id);
                }
            }
            AppToShell::TaskAction { id: task, cancel } => {
                if window.launch.application()
                    == super::window_launch::WindowApplication::Management(ManagementKind::Tasks)
                    && let Some((&owner, _)) = self
                        .windows
                        .iter()
                        .find(|(_, window)| window.status.task_ids.contains(&task))
                    && self.enqueue(owner, ShellToApp::TaskAction { id: task, cancel })
                    && !cancel
                {
                    self.selection.switch(owner);
                    self.cancel_capture();
                }
            }
            AppToShell::ToggleTasks => {
                if self.selection.focused() == Some(id) {
                    self.toggle_tasks(state)?;
                }
            }
            AppToShell::Status(status) => {
                if status.unresponsive {
                    self.input_fences.remove(&id);
                }
                window.update_status(status);
            }
            AppToShell::OpenApp(launch) => {
                if self.selection.focused() == Some(id) && self.pending_global.is_none() {
                    match serde_json::from_value(launch)? {
                        WindowRequest::Open {
                            launch,
                            new_session,
                        } => {
                            self.open_requested(launch, new_session, state.auth_session())?;
                        }
                        WindowRequest::LauncherEntry { id: entry }
                            if window.launch.application()
                                == window_launch::WindowApplication::Launcher
                                && state.can_execute_command_line() =>
                        {
                            if !self.restore_running_window(|launch| {
                                launch.launcher_id() == Some(entry.as_str())
                            }) {
                                self.enqueue(id, ShellToApp::OpenLauncherEntry { id: entry });
                            }
                        }
                        _ => {}
                    }
                }
            }
            AppToShell::BackgroundRequested => {
                self.selection.minimize(id);
                self.cancel_capture();
            }
            AppToShell::CloseRequested => {
                self.request_close(id);
            }
            AppToShell::CloseDeferred => {
                window.close = CloseProgress::Confirmation {
                    size_notice_dismissed: false,
                };
                if self.pending_global.is_some() && self.selection.focused() != Some(id) {
                    self.selection.switch(id);
                }
                self.hide_close_wait(id);
            }
            AppToShell::CloseStarted => {
                window.close = CloseProgress::Cleaning(Instant::now());
                self.input_fences.remove(&id);
            }
            AppToShell::CloseCancelled => {
                window.close = CloseProgress::Idle;
                self.deleting_launcher.remove(&id);
                self.hide_close_wait(id);
                self.pending_global = None;
                self.pending_update = None;
                self.reset = false;
            }
            AppToShell::Closed => {
                window.finish_cleanup();
                self.input_fences.remove(&id);
                self.hide_close_wait(id);
            }
            AppToShell::LaunchFailed(reason) => {
                window.launch_error = true;
                window.close = CloseProgress::Cleaning(Instant::now());
                self.input_fences.remove(&id);
                if let Some((source, launch)) = self.split_launch
                    && launch == id
                {
                    self.selection.minimize(id);
                    self.selection.focus(source);
                    self.split_launch = None;
                }
                report_launch_failure(state, reason);
            }
            AppToShell::ShellAction(HostAction::Logout) => {
                self.pending_global = Some(ShellCommand::LogoutToLockscreen);
            }
            AppToShell::ShellAction(HostAction::Restart) => {
                self.pending_global = Some(ShellCommand::Restart);
            }
            AppToShell::ShellAction(HostAction::Exit) => {
                if self.selection.focused() == Some(id) {
                    state.apply_routed_event(
                        RoutedEvent {
                            input: InputEvent::Tick,
                            target: RoutedTarget::Global,
                            command: ShellCommand::RequestExit,
                        },
                        platform::native_platform().as_ref(),
                        Instant::now(),
                    );
                }
            }
            AppToShell::ShellAction(HostAction::PowerOff) => {
                self.pending_global = Some(ShellCommand::PowerOff);
            }
            AppToShell::ShellAction(HostAction::Reboot) => {
                self.pending_global = Some(ShellCommand::Reboot);
            }
            AppToShell::ShellAction(HostAction::Reset) => self.reset = true,
            AppToShell::ShellAction(HostAction::UpdatePrepared(path)) => {
                self.pending_update = Some(path);
                window.close = CloseProgress::Cleaning(Instant::now());
            }
            AppToShell::ShellAction(HostAction::SettingsChanged) => {}
            AppToShell::ShellAction(HostAction::DebugPanicRequested) => {
                super::super::runtime::trigger_command_line_panic();
            }
            AppToShell::ShellAction(HostAction::AutomationChanged(commit)) => {
                state.record_automation_commit(commit);
            }
            AppToShell::ShellAction(HostAction::LanguageReloaded { code }) => {
                if window.launch.application() == super::window_launch::WindowApplication::Settings
                    && state.language_code() == code
                    && let Ok((catalog, loaded)) = state.prepare_language(&code)
                {
                    state.publish_language(catalog, loaded);
                }
            }
            AppToShell::ShellAction(HostAction::ClockChanged) => {
                if let Some(auth) = state.auth_session().cloned() {
                    state.load_clock_for_session(&auth);
                }
            }
        }
        Ok(true)
    }

    fn sync_task_catalogs(&mut self) {
        let tasks = self
            .windows
            .iter()
            .filter(|(_, window)| {
                window.launch.application()
                    == super::window_launch::WindowApplication::Management(ManagementKind::Tasks)
            })
            .map(|(&id, _)| id)
            .collect::<Vec<_>>();
        if tasks.is_empty() {
            return;
        }
        let ids = self
            .windows
            .values()
            .flat_map(|window| window.status.task_ids.iter().cloned())
            .collect::<Vec<_>>();
        for id in tasks {
            if self.task_catalogs.get(&id) != Some(&ids)
                && self.enqueue(id, ShellToApp::TaskCatalog { ids: ids.clone() })
            {
                self.task_catalogs.insert(id, ids.clone());
            }
        }
    }

    pub(super) fn request_close(&mut self, id: SessionId) {
        let Some(window) = self.windows.get_mut(&id) else {
            return;
        };
        if window.close != CloseProgress::Idle {
            return;
        }
        window.close = CloseProgress::Requested(Instant::now());
        window.queued.push_back(ShellToApp::RequestClose);
        if self.selection.focused() != Some(id) {
            self.selection.activate(id, &self.layout);
        }
        self.panel = None;
        self.close_split_menu();
        self.taskbar_menu = None;
        self.cancel_capture();
    }

    pub(super) fn force(&mut self, id: SessionId) -> io::Result<()> {
        let Some(window) = self.windows.get_mut(&id) else {
            return Ok(());
        };
        if let Some(worker) = window.worker {
            WorkerConnection::current()
                .expect("managed workspace")
                .send(WorkerToSupervisor::ForceKill { worker })?;
            window.state = ProcessState::ForceStopping;
        } else {
            window.force_when_started = true;
        }
        window.close = CloseProgress::Waiting;
        self.hide_close_wait(id);
        self.cancel_capture();
        Ok(())
    }

    pub(super) fn enqueue(&mut self, id: SessionId, mut message: ShellToApp) -> bool {
        if let Some(window) = self.windows.get_mut(&id)
            && window.init.is_none()
            && window.stream.is_some()
            && !matches!(
                window.state,
                ProcessState::Unresponsive | ProcessState::ForceStopping
            )
            && !matches!(
                window.close,
                CloseProgress::Cleaning(_) | CloseProgress::Waiting | CloseProgress::Exiting
            )
        {
            let reserved = message.reserved();
            let messages = if reserved {
                MAX_MESSAGES
            } else {
                MAX_MESSAGES - RESERVED_MESSAGES
            };
            let bytes = if reserved {
                MAX_CONTROL_BYTES
            } else {
                MAX_CONTROL_BYTES - RESERVED_BYTES
            };
            if (!reserved && window.input_blocked)
                || window.queued.len() + usize::from(window.view_requested.is_some()) >= messages
                || window
                    .queued
                    .iter()
                    .map(Message::resident_bytes)
                    .sum::<usize>()
                    + window
                        .view_requested
                        .as_ref()
                        .map_or(0, Message::resident_bytes)
                    + message.resident_bytes()
                    > bytes
            {
                self.input_busy_notice |= !window.input_blocked;
                window.input_blocked = true;
                self.input_rejected = true;
                self.cancel_capture();
                return false;
            }
            if let ShellToApp::Input { serial, .. } = &mut message {
                self.input_serial += 1;
                *serial = self.input_serial;
                self.performance.input_sent(id, *serial);
            }
            if matches!(
                message,
                ShellToApp::Input { .. }
                    | ShellToApp::Back { .. }
                    | ShellToApp::Help { .. }
                    | ShellToApp::Keyboard { .. }
            ) {
                self.input_recipients.insert(id);
            }
            if let Some(viewport) = window.view_requested.take() {
                window.queued.push_back(viewport);
            }
            window.queued.push_back(message);
            true
        } else {
            false
        }
    }
}

fn flush_window_requests(window: &mut Window) {
    let Some(stream) = &mut window.stream else {
        return;
    };
    while let Some(message) = window.queued.pop_front() {
        let initializes = matches!(message, ShellToApp::Init(_));
        match stream.try_queue_owned(message) {
            Ok(()) => {
                if initializes && let Some(startup) = &mut window.startup {
                    startup.init_queued = Some(Instant::now());
                }
            }
            Err((error, message)) if error.kind() == io::ErrorKind::WouldBlock => {
                window.queued.push_front(message);
                break;
            }
            Err((error, mut message)) if error.kind() == io::ErrorKind::InvalidInput => {
                if let ShellToApp::Init(init) = &mut message
                    && init.language.take().is_some()
                {
                    window.queued.push_front(message);
                    continue;
                }
                window.queued.push_front(message);
                break;
            }
            Err((_, message)) => {
                window.queued.push_front(message);
                break;
            }
        }
    }
    if window.queued.is_empty()
        && let Some(message) = &window.view_requested
    {
        match stream.queue(message) {
            Ok(()) => window.view_requested = None,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
            Err(_) => window.view_requested = None,
        }
    }
}

fn report_launch_failure(state: &mut ShellSession, reason: String) {
    state.notify_alert_with_tone(
        i18n::msg!("workspace-launch-failed", reason = reason),
        ui::NotificationTone::Error,
    );
}

impl WindowStartup {
    fn record(
        &self,
        session: SessionId,
        application: &str,
        worker: Option<WorkerId>,
        auth: Option<&AuthSession>,
    ) {
        let accepted = self.accepted.expect("accepted window frame");
        let (Some(app_started), Some(init_queued), Some(app), Some(worker)) =
            (self.app_started, self.init_queued, self.app_timings, worker)
        else {
            return;
        };
        let total_us = accepted.duration_since(self.requested).as_micros() as u64;
        let spawn_app_us = app_started.duration_since(self.requested).as_micros() as u64;
        let handshake_us = init_queued.duration_since(app_started).as_micros() as u64;
        // The IPC legs use different monotonic clocks; subtract only measured durations.
        let app_work_us = app
            .services_setup_us
            .saturating_add(app.resources_us)
            .saturating_add(app.session_init_us)
            .saturating_add(app.first_frame_ready_us);
        let window_ipc_us =
            (accepted.duration_since(init_queued).as_micros() as u64).saturating_sub(app_work_us);
        let mut event = runtime_log::RuntimeLogEvent::new(
            runtime_log::LogContext {
                run_id: Some(worker.run.to_string()),
                app: application.into(),
                module: "ux.workspace".into(),
                operation: "app-startup".into(),
                operation_id: Some(session.to_string()),
                owner_id: auth.map(|auth| auth.user_id.clone()),
                ..Default::default()
            },
            runtime_log::LogLevel::Info,
            runtime_log::LogPhase::Succeeded,
            format!(
                "First window frame accepted in {:.2} ms",
                total_us as f64 / 1_000.0
            ),
        );
        event.message_args = [
            ("total_us", total_us),
            ("spawn_app_us", spawn_app_us),
            ("process_spawn_us", self.process_spawn_us),
            ("guardian_ready_wait_us", self.guardian_ready_wait_us),
            ("handshake_us", handshake_us),
            ("services_setup_us", app.services_setup_us),
            ("resources_us", app.resources_us),
            ("session_init_us", app.session_init_us),
            ("session_build_us", app.session_build_us),
            ("language_load_us", app.language_load_us),
            ("launch_activate_us", app.launch_activate_us),
            ("session_sync_us", app.session_sync_us),
            ("first_frame_ready_us", app.first_frame_ready_us),
            ("first_poll_us", app.first_poll_us),
            ("first_draw_us", app.first_draw_us),
            ("window_ipc_us", window_ipc_us),
        ]
        .into_iter()
        .map(|(key, value)| (key.into(), value.into()))
        .collect();
        runtime_log::record(event);
    }
}
