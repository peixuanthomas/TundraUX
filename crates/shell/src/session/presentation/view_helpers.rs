use super::super::*;
#[cfg(all(test, target_os = "linux"))]
#[path = "../../../tests/unit/session/presentation/view_helpers_tests.rs"]
mod tests;
pub(in crate::session) fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    Rect::new(
        area.x.saturating_add(area.width.saturating_sub(width) / 2),
        area.y
            .saturating_add(area.height.saturating_sub(height) / 2),
        width,
        height,
    )
}

pub(in crate::session) fn clock_display_label(display: time::ClockDisplay) -> String {
    format!(
        "{} {:02}:{:02}",
        display.date,
        display.time.hour(),
        display.time.minute()
    )
}

pub(in crate::session) fn clock_button_active_for_screen(screen: ShellScreen) -> bool {
    matches!(
        screen,
        ShellScreen::Home
            | ShellScreen::Explorer
            | ShellScreen::Launcher
            | ShellScreen::CommandLine
            | ShellScreen::Editor
            | ShellScreen::Settings
            | ShellScreen::SystemStatus
            | ShellScreen::Logs
            | ShellScreen::Management
            | ShellScreen::Diagnostics
            | ShellScreen::Clock
    )
}

pub(in crate::session) fn diagnostics_status_to_ui(
    status: app::diagnostics::DiagnosticStatus,
) -> ui::DiagnosticsStatus {
    match status {
        app::diagnostics::DiagnosticStatus::Pass => ui::DiagnosticsStatus::Pass,
        app::diagnostics::DiagnosticStatus::Unsupported => ui::DiagnosticsStatus::Unsupported,
        app::diagnostics::DiagnosticStatus::Warning => ui::DiagnosticsStatus::Warning,
        app::diagnostics::DiagnosticStatus::Fail => ui::DiagnosticsStatus::Fail,
    }
}

pub(in crate::session) fn diagnostics_incident_severity_to_ui(
    severity: watchdog::IncidentSeverity,
) -> ui::DiagnosticsStatus {
    match severity {
        watchdog::IncidentSeverity::Warning => ui::DiagnosticsStatus::Warning,
        watchdog::IncidentSeverity::Error | watchdog::IncidentSeverity::Critical => {
            ui::DiagnosticsStatus::Fail
        }
    }
}

pub(in crate::session) fn diagnostics_recovery_label(
    recovery: &watchdog::RecoveryOutcome,
) -> String {
    match recovery {
        watchdog::RecoveryOutcome::Pending => i18n::tr!("shell-pending"),
        watchdog::RecoveryOutcome::Recovered(_) => i18n::tr!("shell-recovered"),
        watchdog::RecoveryOutcome::RecoveredWithWarnings(_) => {
            i18n::tr!("shell-recovered-with-warnings")
        }
        watchdog::RecoveryOutcome::ManualActionRequired(_) => {
            i18n::tr!("shell-manual-action-required")
        }
        watchdog::RecoveryOutcome::Unrecoverable(_) => i18n::tr!("shell-unrecoverable"),
    }
}

pub(in crate::session) fn diagnostics_public_check_summary(
    check: &app::diagnostics::DiagnosticCheck,
) -> String {
    use app::diagnostics::{DiagnosticCategory, DiagnosticStatus};

    match (check.category, check.status) {
        (DiagnosticCategory::Environment, DiagnosticStatus::Pass) => {
            i18n::tr!("shell-environment-check-passed")
        }
        (DiagnosticCategory::Environment, DiagnosticStatus::Unsupported) => {
            i18n::tr!("shell-environment-capability-is-unsupported")
        }
        (DiagnosticCategory::Environment, DiagnosticStatus::Warning) => {
            i18n::tr!("shell-environment-check-needs-review")
        }
        (DiagnosticCategory::Environment, DiagnosticStatus::Fail) => {
            i18n::tr!("shell-environment-check-failed")
        }
        (DiagnosticCategory::Paths, DiagnosticStatus::Pass) => {
            i18n::tr!("shell-application-path-is-accessible")
        }
        (DiagnosticCategory::Paths, DiagnosticStatus::Unsupported) => {
            i18n::tr!("shell-application-path-capability-is-unsupported")
        }
        (DiagnosticCategory::Paths, DiagnosticStatus::Warning) => {
            i18n::tr!("shell-application-path-needs-attention")
        }
        (DiagnosticCategory::Paths, DiagnosticStatus::Fail) => {
            i18n::tr!("shell-application-path-check-failed")
        }
        (DiagnosticCategory::Storage, DiagnosticStatus::Pass) => {
            i18n::tr!("shell-storage-document-is-healthy")
        }
        (DiagnosticCategory::Storage, DiagnosticStatus::Unsupported) => {
            i18n::tr!("shell-storage-capability-is-unsupported")
        }
        (DiagnosticCategory::Storage, DiagnosticStatus::Warning) => {
            i18n::tr!("shell-storage-document-needs-attention")
        }
        (DiagnosticCategory::Storage, DiagnosticStatus::Fail) => {
            i18n::tr!("shell-storage-document-check-failed")
        }
        (DiagnosticCategory::Assets, DiagnosticStatus::Pass) => {
            i18n::tr!("shell-required-asset-is-available")
        }
        (DiagnosticCategory::Assets, DiagnosticStatus::Unsupported) => {
            i18n::tr!("shell-required-asset-capability-is-unsupported")
        }
        (DiagnosticCategory::Assets, DiagnosticStatus::Warning) => {
            i18n::tr!("shell-required-asset-needs-attention")
        }
        (DiagnosticCategory::Assets, DiagnosticStatus::Fail) => {
            i18n::tr!("shell-required-asset-check-failed")
        }
    }
}

pub(in crate::session) fn explorer_system_time_label(
    value: SystemTime,
    zone: storage::ExplorerDateZone,
    configured_timezone: Option<&str>,
) -> String {
    let utc = DateTime::<Utc>::from(value);
    match zone {
        storage::ExplorerDateZone::Utc => utc.format("%Y-%m-%d %H:%M").to_string(),
        storage::ExplorerDateZone::ConfiguredTimezone if configured_timezone.is_none() => utc
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M")
            .to_string(),
        storage::ExplorerDateZone::ConfiguredTimezone => configured_timezone
            .and_then(|timezone| timezone.parse::<chrono_tz::Tz>().ok())
            .map(|timezone| {
                utc.with_timezone(&timezone)
                    .format("%Y-%m-%d %H:%M")
                    .to_string()
            })
            .unwrap_or_else(|| utc.format("%Y-%m-%d %H:%M").to_string()),
    }
}

pub(in crate::session) fn explorer_size_label(
    size: u64,
    format: storage::ExplorerSizeFormat,
) -> String {
    if format == storage::ExplorerSizeFormat::Bytes {
        return format!("{size} B");
    }
    const UNITS: &[&str] = &["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = size as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", size, UNITS[unit])
    } else if value >= 10.0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

pub(in crate::session) fn explorer_display_name(
    entry: &app::explorer::ExplorerEntry,
    show_extensions: bool,
) -> String {
    if show_extensions || entry.kind != app::explorer::ExplorerEntryKind::File {
        return entry.name.clone();
    }
    entry
        .path
        .file_stem()
        .and_then(std::ffi::OsStr::to_str)
        .filter(|stem| !stem.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| entry.name.clone())
}

pub(in crate::session) fn explorer_breadcrumb_view_models(
    path: &std::path::Path,
    state: &ExplorerState,
) -> Vec<ui::ExplorerBreadcrumbViewModel> {
    let mut ancestors = path.ancestors().collect::<Vec<_>>();
    ancestors.reverse();
    ancestors
        .into_iter()
        .enumerate()
        .map(|(index, ancestor)| {
            let label = ancestor
                .file_name()
                .and_then(std::ffi::OsStr::to_str)
                .filter(|label| !label.is_empty())
                .map(str::to_string)
                .unwrap_or_else(|| ancestor.display().to_string());
            let mut model = ui::ExplorerBreadcrumbViewModel::new(
                format!("breadcrumb-{index}"),
                label,
                ancestor.display().to_string(),
            );
            model.drop_target = state
                .drag
                .as_ref()
                .and_then(|drag| drag.target.as_ref())
                .is_some_and(|target| target == ancestor);
            model
        })
        .collect()
}

pub(in crate::session) struct ExplorerContextMenuInput {
    pub(in crate::session) anchor: CellPosition,
    pub(in crate::session) centered: bool,
    pub(in crate::session) selected_count: usize,
    pub(in crate::session) entry_count: usize,
    pub(in crate::session) clipboard_available: bool,
    pub(in crate::session) undo_available: bool,
    pub(in crate::session) is_trash: bool,
    pub(in crate::session) trash_has_items: bool,
    pub(in crate::session) focused_index: usize,
    pub(in crate::session) can_manage_launcher: bool,
    pub(in crate::session) can_execute_command_line: bool,
    pub(in crate::session) launcher_eligible_count: usize,
}

pub(in crate::session) fn explorer_context_menu_view_model(
    input: ExplorerContextMenuInput,
) -> ui::ExplorerOverlayViewModel {
    let ExplorerContextMenuInput {
        anchor,
        centered,
        selected_count,
        entry_count,
        clipboard_available,
        undo_available,
        is_trash,
        trash_has_items,
        focused_index,
        can_manage_launcher,
        can_execute_command_line,
        launcher_eligible_count,
    } = input;
    let item = |id: &str, label: String, enabled: bool, dangerous: bool| {
        ui::ExplorerContextMenuItemViewModel {
            id: id.to_string(),
            label: label.to_string(),
            shortcut: None,
            enabled,
            dangerous,
        }
    };
    let mut items = if is_trash && selected_count > 0 {
        vec![
            item(
                "restore",
                i18n::tr!("shell-restore"),
                selected_count == 1,
                false,
            ),
            item(
                "properties",
                i18n::tr!("shell-properties"),
                selected_count > 0,
                false,
            ),
        ]
    } else if is_trash {
        vec![
            item("refresh", i18n::tr!("shell-refresh"), true, false),
            item(
                "dump-trash",
                i18n::tr!("shell-dump-trash"),
                trash_has_items,
                true,
            ),
            item("sort", i18n::tr!("shell-sort"), true, false),
            item("options", i18n::tr!("shell-advanced-options"), true, false),
        ]
    } else if selected_count > 0 {
        let mut items = vec![item(
            "open",
            i18n::tr!("shell-open"),
            selected_count == 1,
            false,
        )];
        if can_manage_launcher && launcher_eligible_count > 0 {
            items.push(item(
                "add-to-launcher",
                i18n::tr!("shell-add-to-launcher"),
                true,
                false,
            ));
        }
        items.extend([
            item("cut", i18n::tr!("shell-cut"), true, false),
            item("copy", i18n::tr!("shell-copy"), true, false),
            item(
                "rename",
                i18n::tr!("shell-rename"),
                selected_count == 1,
                false,
            ),
            item("delete", i18n::tr!("shell-delete"), true, true),
            item(
                "properties",
                i18n::tr!("shell-properties"),
                selected_count > 0,
                false,
            ),
        ]);
        items
    } else {
        vec![
            item("new-folder", i18n::tr!("shell-new-folder"), true, false),
            item("new-text", i18n::tr!("shell-new-text-file"), true, false),
            item(
                "paste",
                i18n::tr!("shell-paste"),
                clipboard_available,
                false,
            ),
            item(
                "open-terminal",
                i18n::tr!("ui-explorer-open-terminal-here"),
                can_execute_command_line,
                false,
            ),
            item("refresh", i18n::tr!("shell-refresh"), true, false),
            item("sort", i18n::tr!("shell-sort"), true, false),
            item("options", i18n::tr!("shell-advanced-options"), true, false),
        ]
    };
    if !is_trash && selected_count > 0 {
        items.push(item(
            "paste",
            i18n::tr!("shell-paste"),
            clipboard_available,
            false,
        ));
    }
    items.push(item(
        "undo",
        i18n::tr!("ui-explorer-undo-move"),
        undo_available,
        false,
    ));
    if selected_count > 0 {
        items.extend([
            item("sort", i18n::tr!("shell-sort"), true, false),
            item("options", i18n::tr!("shell-advanced-options"), true, false),
        ]);
    }
    if centered {
        if selected_count == 0 {
            items.push(item("properties", i18n::tr!("shell-properties"), false, false));
        }
        items.sort_by_key(|item| match item.id.as_str() {
            "sort" => 0,
            "properties" => 1,
            "options" => 2,
            _ => 3,
        });
    }
    items.extend([
        item(
            "select-all",
            i18n::tr!("shell-select-all"),
            entry_count > 0,
            false,
        ),
        item(
            "invert-selection",
            i18n::tr!("ui-explorer-invert-selection"),
            entry_count > 0,
            false,
        ),
        item(
            "clear-selection",
            i18n::tr!("ui-explorer-clear-selection"),
            selected_count > 0,
            false,
        ),
    ]);
    let selected_index = if items.get(focused_index).is_some_and(|item| item.enabled) {
        Some(focused_index)
    } else {
        items.iter().position(|item| item.enabled)
    };
    ui::ExplorerOverlayViewModel::ContextMenu(ui::ExplorerContextMenuViewModel {
        x: anchor.0,
        y: anchor.1,
        centered,
        title: if centered {
            i18n::tr!("ui-explorer-actions-menu")
        } else if selected_count > 0 {
            i18n::tr!("shell-selection")
        } else {
            i18n::tr!("shell-explorer")
        },
        items,
        selected_index,
    })
}

pub(in crate::session) fn explorer_sort_menu_view_model(
    anchor: CellPosition,
    selected: ui::ExplorerSortColumn,
    focused_index: usize,
) -> ui::ExplorerOverlayViewModel {
    let items = ui::ExplorerSortColumn::ALL
        .into_iter()
        .map(|column| ui::ExplorerContextMenuItemViewModel {
            id: match column {
                ui::ExplorerSortColumn::Name => "sort-name",
                ui::ExplorerSortColumn::Type => "sort-type",
                ui::ExplorerSortColumn::Size => "sort-size",
                ui::ExplorerSortColumn::Modified => "sort-modified",
            }
            .to_string(),
            label: if column == selected {
                format!("* {}", column.label())
            } else {
                format!("  {}", column.label())
            },
            shortcut: None,
            enabled: true,
            dangerous: false,
        })
        .collect();
    ui::ExplorerOverlayViewModel::ContextMenu(ui::ExplorerContextMenuViewModel {
        x: anchor.0,
        y: anchor.1,
        centered: false,
        title: i18n::tr!("shell-sort-by"),
        items,
        selected_index: Some(focused_index.min(ui::ExplorerSortColumn::ALL.len() - 1)),
    })
}

pub(in crate::session) fn explorer_options_view_model(
    state: &ExplorerState,
    focused_index: usize,
    enabled: bool,
) -> ui::ExplorerOverlayViewModel {
    let defaults = storage::ExplorerConfig::default();
    let toggle =
        |id: &str, label: String, value: bool, default: bool| ui::ExplorerOptionViewModel {
            id: id.to_string(),
            label: label.to_string(),
            value: if value {
                i18n::tr!("shell-on")
            } else {
                i18n::tr!("shell-off")
            }
            .to_string(),
            enabled,
            modified: value != default,
            focused: false,
        };
    let mut options = vec![
        toggle(
            "hidden",
            i18n::tr!("shell-show-hidden-files"),
            state.show_hidden,
            defaults.show_hidden,
        ),
        toggle(
            "system",
            i18n::tr!("shell-show-system-files"),
            state.show_system,
            defaults.show_system,
        ),
        toggle(
            "extensions",
            i18n::tr!("shell-show-file-extensions"),
            state.show_extensions,
            defaults.show_extensions,
        ),
        toggle(
            "folders-first",
            i18n::tr!("shell-folders-first"),
            state.folders_first,
            defaults.folders_first,
        ),
        toggle(
            "case-sensitive",
            i18n::tr!("shell-case-sensitive-sort"),
            state.case_sensitive_sort,
            defaults.case_sensitive_sort,
        ),
        ui::ExplorerOptionViewModel {
            id: "size-format".to_string(),
            label: i18n::tr!("shell-size-format"),
            value: match state.size_format {
                storage::ExplorerSizeFormat::HumanBinary => i18n::tr!("shell-human-binary"),
                storage::ExplorerSizeFormat::Bytes => i18n::tr!("shell-bytes"),
            }
            .to_string(),
            enabled,
            modified: state.size_format != defaults.size_format,
            focused: false,
        },
        ui::ExplorerOptionViewModel {
            id: "date-zone".to_string(),
            label: i18n::tr!("shell-date-zone"),
            value: match state.date_zone {
                storage::ExplorerDateZone::ConfiguredTimezone => i18n::tr!("shell-configured"),
                storage::ExplorerDateZone::Utc => "UTC".to_string(),
            }
            .to_string(),
            enabled,
            modified: state.date_zone != defaults.date_zone,
            focused: false,
        },
        toggle(
            "confirm-delete",
            i18n::tr!("shell-confirm-delete"),
            state.confirm_delete,
            defaults.confirm_delete,
        ),
        toggle(
            "confirm-conflicts",
            i18n::tr!("shell-confirm-name-conflicts"),
            state.confirm_name_conflicts,
            defaults.confirm_name_conflicts,
        ),
        toggle(
            "sidebar",
            i18n::tr!("shell-show-quick-access"),
            state.show_sidebar,
            defaults.show_sidebar,
        ),
    ];
    if let Some(option) = options.get_mut(focused_index) {
        option.focused = true;
    }
    ui::ExplorerOverlayViewModel::Options(ui::ExplorerOptionsViewModel {
        title: i18n::tr!("shell-advanced-options"),
        options,
        close_label: i18n::tr!("shell-close"),
    })
}

pub(in crate::session) fn explorer_properties_view_model(
    state: &ExplorerState,
    configured_timezone: Option<&str>,
) -> ui::ExplorerOverlayViewModel {
    let Some(entry) = state.single_selected_entry() else {
        return ui::ExplorerOverlayViewModel::Properties(ui::ExplorerPropertiesViewModel {
            title: i18n::tr!("shell-properties"),
            properties: vec![ui::ExplorerPropertyViewModel {
                label: i18n::tr!("shell-selection"),
                value: if state.effective_selected_paths().is_empty() {
                    i18n::tr!("shell-no-item-selected")
                } else {
                    i18n::tr!(
                        "permissions-selection",
                        count = state.effective_selected_paths().len() as i64
                    )
                },
            }],
            close_label: i18n::tr!("shell-close"),
        });
    };
    let mut properties = vec![
        ui::ExplorerPropertyViewModel {
            label: i18n::tr!("shell-name"),
            value: entry.name.clone(),
        },
        ui::ExplorerPropertyViewModel {
            label: i18n::tr!("shell-path"),
            value: entry.path.display().to_string(),
        },
        ui::ExplorerPropertyViewModel {
            label: i18n::tr!("shell-type"),
            value: entry.localized_type_label().render_current(),
        },
        ui::ExplorerPropertyViewModel {
            label: i18n::tr!("shell-size"),
            value: if entry.kind == app::explorer::ExplorerEntryKind::Directory {
                "--".to_string()
            } else {
                explorer_size_label(entry.size, state.size_format)
            },
        },
        ui::ExplorerPropertyViewModel {
            label: i18n::tr!("shell-modified"),
            value: entry
                .modified
                .map(|modified| {
                    explorer_system_time_label(modified, state.date_zone, configured_timezone)
                })
                .unwrap_or_else(|| i18n::tr!("shell-unknown")),
        },
        ui::ExplorerPropertyViewModel {
            label: i18n::tr!("shell-attributes"),
            value: {
                let labels = explorer_attribute_labels(&entry.attributes);
                if labels.is_empty() {
                    i18n::tr!("shell-none")
                } else {
                    labels.join(", ")
                }
            },
        },
    ];
    if let Some(reason) = entry.open_policy.reason() {
        properties.push(ui::ExplorerPropertyViewModel {
            label: i18n::tr!("shell-open-policy"),
            value: reason.to_string(),
        });
    }
    ui::ExplorerOverlayViewModel::Properties(ui::ExplorerPropertiesViewModel {
        title: i18n::tr!("shell-properties-arg1", arg1 = &entry.name),
        properties,
        close_label: i18n::tr!("shell-close"),
    })
}

pub(in crate::session) fn explorer_attribute_labels(attributes: &FileAttributes) -> Vec<String> {
    let mut labels = Vec::new();
    if attributes.readonly {
        labels.push(i18n::tr!("shell-readonly"));
    }
    if attributes.hidden {
        labels.push(i18n::tr!("shell-hidden"));
    }
    if attributes.system {
        labels.push(i18n::tr!("shell-system"));
    }
    if attributes.archive {
        labels.push(i18n::tr!("shell-archive"));
    }
    if attributes.symlink {
        labels.push(i18n::tr!("shell-symlink"));
    }
    if attributes.junction {
        labels.push(i18n::tr!("shell-junction"));
    }
    if attributes.reparse_point {
        labels.push(i18n::tr!("shell-reparse"));
    }
    if attributes.shortcut {
        labels.push(i18n::tr!("shell-shortcut"));
    }
    labels
}

pub(in crate::session) fn explorer_input_prompt(mode: ExplorerInputMode) -> String {
    match mode {
        ExplorerInputMode::Browse => i18n::tr!("shell-explorer"),
        ExplorerInputMode::Address => i18n::tr!("shell-absolute-path"),
        ExplorerInputMode::Search => i18n::tr!("shell-search"),
        ExplorerInputMode::NewFolder => i18n::tr!("shell-new-folder-name"),
        ExplorerInputMode::NewTextFile => i18n::tr!("shell-new-text-file-name"),
        ExplorerInputMode::Rename => i18n::tr!("shell-rename-to"),
        ExplorerInputMode::RestoreDestination => i18n::tr!("shell-restore-destination-directory"),
    }
}

pub(in crate::session) fn terminal_flag_labels(flags: ShellTerminalFlags) -> Vec<String> {
    let mut labels = Vec::new();

    if flags.raw_mode {
        labels.push(i18n::tr!("shell-raw-mode-enabled"));
    }
    if flags.alternate_screen {
        labels.push(i18n::tr!("shell-alternate-screen-enabled"));
    }
    if flags.mouse_capture {
        labels.push(i18n::tr!("shell-mouse-capture-enabled"));
    }
    if flags.cursor_restore_enabled {
        labels.push(i18n::tr!("shell-cursor-restore-enabled"));
    }

    labels
}

pub(in crate::session) fn resolved_home_mode(
    launch_config: ShellLaunchConfig,
    startup: &ShellStartupState,
) -> ShellHomeMode {
    let requested_mode = match launch_config.home_mode_override {
        HomeModeOverride::Debug => ShellHomeMode::Debug,
        HomeModeOverride::BuildDefault => startup
            .restored_session
            .as_ref()
            .map(|session| session.display_mode)
            .or(startup.app_config.home_mode)
            .unwrap_or_else(|| ShellSession::legacy_default_home_mode(launch_config)),
    };

    if requested_mode == ShellHomeMode::Debug && !startup.debug_policy.allows_debug() {
        ShellHomeMode::User
    } else {
        requested_mode
    }
}

pub(in crate::session) fn platform_capability_summary(
    kind: PlatformKind,
    capabilities: &PlatformCapabilities,
) -> i18n::LocalizedText {
    let (mut supported, mut best_effort, mut unsupported) = (0, 0, 0);

    for (_, status) in capabilities.checks() {
        match status {
            CapabilityStatus::Supported => supported += 1,
            CapabilityStatus::BestEffort => best_effort += 1,
            CapabilityStatus::Unsupported => unsupported += 1,
        }
    }

    i18n::msg!(
        "shell-arg1-supported-supported-best-effort-best-effort-unsupported-unsupported",
        arg1 = kind.as_str(),
        supported = supported,
        best_effort = best_effort,
        unsupported = unsupported
    )
    .into()
}

pub(in crate::session) fn build_mode_label() -> &'static str {
    if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    }
}

pub(in crate::session) fn component_message(component: ShellComponent) -> i18n::LocalizedMessage {
    match component {
        ShellComponent::CompactHome => i18n::msg!("shell-component-compact-home"),
        ShellComponent::TopBar => i18n::msg!("shell-component-top-bar"),
        ShellComponent::BackButton => i18n::msg!("shell-component-back-button"),
        ShellComponent::HelpButton => i18n::msg!("shell-component-help-button"),
        ShellComponent::HelpDialog => i18n::msg!("shell-component-help-dialog"),
        ShellComponent::Home => i18n::msg!("shell-component-home"),
        ShellComponent::ClockButton => i18n::msg!("shell-component-clock-button"),
        ShellComponent::Clock => i18n::msg!("shell-component-clock"),
        ShellComponent::ClockNewButton => i18n::msg!("shell-component-clock-new-button"),
        ShellComponent::ClockEntryList => i18n::msg!("shell-component-clock-entry-list"),
        ShellComponent::ClockCreateDialog => i18n::msg!("shell-component-clock-create-dialog"),
        ShellComponent::ClockCreateInput => i18n::msg!("shell-component-clock-create-input"),
        ShellComponent::ClockCreateAlarmButton => {
            i18n::msg!("shell-component-clock-create-alarm-button")
        }
        ShellComponent::ClockCreateCountdownButton => {
            i18n::msg!("shell-component-clock-create-countdown-button")
        }
        ShellComponent::ClockCreateCancelButton => i18n::msg!("ui-clock-touch-cancel"),
        ShellComponent::Diagnostics => i18n::msg!("shell-component-diagnostics"),
        ShellComponent::Logs => i18n::msg!("shell-component-logs"),
        ShellComponent::Management => i18n::msg!("management-title"),
        ShellComponent::SystemStatus => i18n::msg!("shell-component-system-status"),
        ShellComponent::DiagnosticsRepairDialog => {
            i18n::msg!("shell-component-diagnostics-repair-dialog")
        }
        ShellComponent::HomeEdit => i18n::msg!("ui-home-edit"),
        ShellComponent::SetupLanguage => i18n::msg!("shell-component-setup-language"),
        ShellComponent::SetupTimezone => i18n::msg!("shell-component-setup-timezone"),
        ShellComponent::SetupAppearanceShape => {
            i18n::msg!("shell-component-setup-appearance-shape")
        }
        ShellComponent::SetupAppearanceThemeColor => {
            i18n::msg!("shell-component-setup-appearance-theme-color")
        }
        ShellComponent::SetupAppearanceThemeCustom => {
            i18n::msg!("shell-component-setup-appearance-theme-custom")
        }
        ShellComponent::SetupAppearanceAccentColor => {
            i18n::msg!("shell-component-setup-appearance-accent-color")
        }
        ShellComponent::SetupAppearanceAccentCustom => {
            i18n::msg!("shell-component-setup-appearance-accent-custom")
        }
        ShellComponent::SetupAppearanceSubmit => {
            i18n::msg!("shell-component-setup-appearance-submit")
        }
        ShellComponent::SetupCustomColorDialog => {
            i18n::msg!("shell-component-setup-custom-color-dialog")
        }
        ShellComponent::Explorer => i18n::msg!("shell-component-explorer"),
        ShellComponent::Launcher => i18n::msg!("shell-component-launcher"),
        ShellComponent::CommandLine => i18n::msg!("shell-component-command-line"),
        ShellComponent::Editor => i18n::msg!("shell-component-editor"),
        ShellComponent::Settings => i18n::msg!("shell-component-settings"),
        ShellComponent::StatusBar => i18n::msg!("shell-component-status-bar"),
        ShellComponent::ExitDialog => i18n::msg!("shell-component-exit-dialog"),
        ShellComponent::TimeSyncDialog => i18n::msg!("shell-component-time-sync-dialog"),
        ShellComponent::NotificationDialog => i18n::msg!("shell-component-notification-dialog"),
        ShellComponent::ContextMenu => i18n::msg!("shell-component-context-menu"),
    }
}
