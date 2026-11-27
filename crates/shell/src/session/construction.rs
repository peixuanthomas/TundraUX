use super::*;
impl ShellSession {
    pub fn new(launch_config: ShellLaunchConfig, terminal_size: (u16, u16)) -> Self {
        Self::new_with_startup(
            launch_config,
            terminal_size,
            ShellStartupState::current_process_defaults(),
        )
    }

    pub fn try_new(
        launch_config: ShellLaunchConfig,
        terminal_size: (u16, u16),
    ) -> Result<Self, ui::AssetError> {
        Self::try_new_with_startup(
            launch_config,
            terminal_size,
            ShellStartupState::current_process_defaults(),
        )
    }

    pub fn new_with_startup(
        launch_config: ShellLaunchConfig,
        terminal_size: (u16, u16),
        startup: ShellStartupState,
    ) -> Self {
        let ascii_assets =
            ui::RuntimeAsciiAssets::load_default().expect("default ASCII assets must load");
        Self::new_with_startup_and_assets(launch_config, terminal_size, startup, ascii_assets)
    }

    pub fn try_new_with_startup(
        launch_config: ShellLaunchConfig,
        terminal_size: (u16, u16),
        startup: ShellStartupState,
    ) -> Result<Self, ui::AssetError> {
        let ascii_assets = ui::RuntimeAsciiAssets::load_default()?;
        Ok(Self::new_with_startup_and_assets(
            launch_config,
            terminal_size,
            startup,
            ascii_assets,
        ))
    }

    pub fn new_with_startup_and_assets(
        launch_config: ShellLaunchConfig,
        terminal_size: (u16, u16),
        startup: ShellStartupState,
        ascii_assets: ui::RuntimeAsciiAssets,
    ) -> Self {
        let explorer_task_runtime = startup
            .storage_manager
            .as_ref()
            .map(|storage| ShellExplorerTaskRuntime::new(storage.clone()));
        let diagnostics_task_runtime = startup
            .storage_manager
            .as_ref()
            .map(|storage| ShellDiagnosticsTaskRuntime::new(storage.clone()));
        let task_watchdog = default_editor_watchdog();
        let editor_task_runtime = task_watchdog.clone().map_or_else(
            ShellEditorTaskRuntime::unavailable,
            ShellEditorTaskRuntime::new_managed,
        );
        let settings_task_runtime = task_watchdog.map_or_else(
            ShellSettingsTaskRuntime::unavailable,
            ShellSettingsTaskRuntime::new_managed,
        );
        Self::new_with_runtime_services(
            launch_config,
            terminal_size,
            startup,
            ascii_assets,
            ShellRuntimeServices {
                language: None,
                terminal_image_support: false,
                explorer: explorer_task_runtime,
                diagnostics: diagnostics_task_runtime,
                editor: editor_task_runtime,
                settings: settings_task_runtime,
                window_owner: Default::default(),
                inherited_auth: None,
                session_snapshot: None,
            },
            None,
        )
    }

    pub(in crate::session) fn new_with_runtime_services(
        launch_config: ShellLaunchConfig,
        terminal_size: (u16, u16),
        startup: ShellStartupState,
        ascii_assets: ui::RuntimeAsciiAssets,
        runtime_services: ShellRuntimeServices,
        language_load_us: Option<&mut u64>,
    ) -> Self {
        let diagnostics_restart_required = runtime_services
            .diagnostics
            .as_ref()
            .is_some_and(ShellDiagnosticsTaskRuntime::restart_required);
        let home_mode = resolved_home_mode(launch_config, &startup);
        let mut storage_config = if let Some(snapshot) = &runtime_services.session_snapshot {
            let (revision, mut config): (_, storage::StorageConfig) =
                crate::session_services::SessionServicesClient::decode(&snapshot.config)
                    .expect("AppHost startup configuration was validated");
            config.session_revision = Some(revision);
            config.session_authorization_revision = snapshot
                .authorization
                .as_ref()
                .ok()
                .map(|auth| auth.revision);
            config
        } else {
            startup
                .storage_manager
                .as_ref()
                .and_then(|storage| storage.load_config().ok())
                .unwrap_or_default()
        };
        if startup.platform_kind == PlatformKind::Linux {
            storage_config.time_sync.source = storage::TimeSyncSource::OperatingSystem;
            storage_config.time_sync.server_url = None;
        }
        let language_started = language_load_us.as_ref().map(|_| Instant::now());
        let prepared_language = runtime_services.language.unwrap_or_else(|| {
            PreparedLanguage::load(ascii_assets.store().root(), &storage_config.language)
        });
        if let (Some(duration), Some(started)) = (language_load_us, language_started) {
            *duration = started.elapsed().as_micros() as u64;
        }
        let language_catalog = prepared_language.catalog;
        let language = prepared_language.snapshot;
        let _language = i18n::enter_snapshot(language.clone());
        let mut app = app::AppState::with_storage_config(storage_config);
        if startup.platform_kind == PlatformKind::Linux {
            app.dispatch_at(
                app::AppCommand::SetClockTimezone {
                    timezone_id: None,
                    synchronized_utc: None,
                },
                Instant::now(),
            );
        }
        if startup.storage_report.has_recovery_warnings() {
            app.dispatch_at(
                app::AppCommand::Notification(app::NotificationCommand::ShowToast(
                    i18n::msg!("startup-storage-recovered").into(),
                )),
                Instant::now(),
            );
        }

        let ui = UiSessionState {
            pages: ShellPages::default(),
            window_owner: runtime_services.window_owner,
            window_launches: VecDeque::new(),
            editor_file_picker: None,
            window_picked_file: None,
            window_delete_launcher: None,
            window_close: Default::default(),
            window_close_explicit: false,
            window_global_request: None,
            window_language_reloaded: None,
            automation_commit: None,
            windows_active: false,
            window_task_ids: HashSet::new(),
            window_task_action: None,
            window_tasks_requested: false,
            command_line_close_request: None,
            script_requests: VecDeque::new(),
            editor_vim_terminal: None,
            editor_vim_install: None,
            editor_vim_toggle_requested: false,
            editor_vim_enabled: None,
            editor_vim_script_export: None,
            editor_vim_close_requested: false,
            language,
            language_catalog,
            repaired_resource_paths: Vec::new(),
            fallback_resource_paths: Vec::new(),
            home_mode,
            ascii_assets,
            navigation: ShellNavigation::new(ShellScreen::Home),
            storage_manager: startup.storage_manager.clone(),
            last_time_sync_utc: None,
            clock_scheduler: None,
            workspace_clock: Default::default(),

            clock_persist_pending: false,
            clock_pending_due_summary: None,
            clock_profile_pending_sync: None,
            time_sync_attempted: false,
            time_sync_dialog_visible: false,
            help_open: false,
            help_scroll: 0,
            help_scrollbar_drag: None,
            time_sync_failure_message: None,
            debug_home: launch_config.home_mode_override == HomeModeOverride::Debug,
            debug_policy: startup.debug_policy,
            platform_kind: startup.platform_kind,

            pending_personalization_session: None,

            system_settings_backend: app::system_settings::UnavailableSystemSettingsBackend {
                reason: if startup.platform_kind == PlatformKind::Linux {
                    app::system_settings::UnavailableReason::NotIntegrated
                } else {
                    app::system_settings::UnavailableReason::UnsupportedPlatform
                },
            },
            tasks: ShellTasks::with_history(
                startup
                    .storage_manager
                    .as_ref()
                    .map(|storage| storage.layout().data_path.join("task-history")),
            ),
            pending_page_closes: HashSet::new(),
            pending_session_exit: None,
            approved_editor_closes: BTreeMap::new(),
            settings_update_request: None,
            settings_task_runtime: runtime_services.settings,
            settings_update_state: SettingsUpdateState::default(),
            update_apply_manifest: None,
            update_restart_requested: false,

            system_status_history: SystemStatusMetricHistory::default(),

            system_status_storage_alerts: BTreeMap::new(),
            system_status_network_baseline: None,
            system_status_disconnected_notified: false,

            launcher_task_runtime: None,

            explorer_task_runtime: runtime_services.explorer,
            editor_task_runtime: runtime_services.editor,

            diagnostics_task_runtime: runtime_services.diagnostics,

            auto_admin: AutoAdminState::default(),
            screen_keyboard: Default::default(),
            details_motion: None,
            #[cfg(target_os = "linux")]
            privilege_session: Default::default(),
            detached_management_operations: Vec::new(),

            diagnostics_restart_required,
            terminal_size,
            terminal_flags: ShellTerminalFlags::enabled(),
            default_icon_display_mode: storage::IconDisplayMode::default_for_platform(
                startup.platform_kind,
            ),
            terminal_image_support: runtime_services.terminal_image_support,
            terminal_image_cell_size: [8, 16],
            editor_media_colors: ui::ThemeTokens::glacier_night().editor_media_colors(),
            terminal_text_sizing_support: false,

            interaction: Default::default(),
            screen_saver: Default::default(),
            button_regions: Vec::new(),
            notification_scrollbar_drag: None,

            hovered_component: None,

            frame_layout: None,
            displayed_status: None,
            notification_history: Default::default(),
            hit_map: ShellHitMap::empty(terminal_size),
            hit_map_generation: 0,
            overlay_interaction_ready: true,
            tick_count: 0,
            notification_bindings: NotificationBindings::default(),
            notification_pointer_capture: None,
            pending_notification_commands: VecDeque::new(),

            latest_watchdog_report: None,
            latest_watchdog_summary: None,
            shutdown_requested: false,
            restart_requested: false,
            last_command: None,
            last_routed_target: None,
            last_key_event: None,
            last_mouse_event: None,
            last_resize_event: None,
            mouse_scroll_direction: None,
            mouse_drag_direction: None,
            platform_capability_summary: platform_capability_summary(
                startup.platform_kind,
                &startup.platform_capabilities,
            ),
            last_click: None,
            drag_tracker: None,
            scrollbar_drag: None,
        };
        let mut state = Self {
            app,
            ui,
            text_clipboard: Default::default(),
        };
        state.initialize_root_page();
        state.report_language_diagnostics(&prepared_language.diagnostics);
        if let Some(target) = std::env::var_os(app::update::UPDATE_TARGET_SHA_ENV) {
            let target = target.to_string_lossy();
            state.notify_toast(i18n::msg!(
                "startup-updated",
                revision = target.chars().take(7).collect::<String>()
            ));
        }
        if let Some(reason) = std::env::var_os(app::update::UPDATE_ROLLBACK_ENV) {
            state.notify_alert_with_tone(
                i18n::msg!(
                    "startup-update-restored",
                    reason = reason.to_string_lossy().to_string()
                ),
                ui::NotificationTone::Error,
            );
        }
        if state.window_owner.is_app() {
            if let Some(session) = runtime_services.inherited_auth {
                state.attach_window_profile(session, runtime_services.session_snapshot.as_deref());
            }
        } else if let Some(session) = startup.system_session {
            state.attach_system_session(session);
        } else if let Some(restored_session) = startup.restored_session.as_ref() {
            state.apply_restored_session(restored_session);
        }
        state.refresh_hit_map();
        state
    }

    fn attach_window_profile(
        &mut self,
        session: AuthSession,
        snapshot: Option<&workspace_protocol::ServiceSessionSnapshot>,
    ) {
        if let Some(snapshot) = snapshot {
            if let Ok((_, profile)) =
                crate::session_services::SessionServicesClient::decode(&snapshot.profile)
            {
                self.apply_workspace_profile(profile);
            }
        } else {
            let profile = self
                .storage_manager
                .as_ref()
                .and_then(|storage| storage.load_profile(&session.username).ok().flatten());
            if let Some(profile) = profile {
                self.app.dispatch_at(
                    app::AppCommand::SetActiveAppearance(Some(profile.appearance)),
                    Instant::now(),
                );
                self.app.dispatch_at(
                    app::AppCommand::SetActiveSystemStatusDashboard(Some(
                        profile.system_status_dashboard,
                    )),
                    Instant::now(),
                );
            }
        }
        self.reset_navigation(ShellScreen::Home);
        self.app.dispatch_at(
            app::AppCommand::SetAuthSession(Some(session.clone())),
            Instant::now(),
        );
        if self.window_owner
            == window_launch::WindowOwner::App(window_launch::WindowApplication::Clock)
            && crate::session_services::SessionServicesClient::current().is_none()
        {
            self.load_clock_for_session(&session);
        }
    }

    pub fn sanitized_session_state(&self) -> ShellRestoredSession {
        let (focus_manager, focus_order) =
            self.focus_manager(Some(self.pages.common.focused_component));
        let focused_component = self
            .focused_component_from_manager(&focus_manager, &focus_order)
            .unwrap_or(ShellComponent::Home);

        ShellRestoredSession {
            active_screen: ShellScreen::Home,
            focused_component,
            display_mode: self.home_mode,
            active_popup: None,
        }
    }

    pub(in crate::session) fn legacy_default_home_mode(
        launch_config: ShellLaunchConfig,
    ) -> ShellHomeMode {
        match launch_config.home_mode_override {
            HomeModeOverride::Debug => ShellHomeMode::Debug,
            HomeModeOverride::BuildDefault => {
                if cfg!(debug_assertions) {
                    ShellHomeMode::Debug
                } else {
                    ShellHomeMode::User
                }
            }
        }
    }

    pub fn new_for_home_mode(
        launch_config: ShellLaunchConfig,
        terminal_size: (u16, u16),
        home_mode: ShellHomeMode,
    ) -> Self {
        let mut state = Self::new(launch_config, terminal_size);
        state.home_mode = home_mode;
        state
    }
}
