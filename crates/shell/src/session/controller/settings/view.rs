use super::*;
use crate::session::*;

impl ShellSession {
    pub fn to_settings_view_model(&self) -> Option<ui::SettingsViewModel> {
        let _language = i18n::enter_snapshot(self.language.clone());
        let state = self.pages.settings.settings_state.as_ref()?;
        let config = self.app.storage_config();
        let appearance = self.app.active_appearance()?;
        let global_enabled = self.can_change_global_settings();
        let identity = app::update::current_build_identity();
        let unavailable_reason = self.system_settings_unavailable_reason(state.category);
        let (cards, update) = if state.category == ui::SettingsCategory::Update {
            let update_platform = if cfg!(target_os = "linux") {
                PlatformKind::Linux
            } else if cfg!(windows) {
                PlatformKind::Windows
            } else {
                PlatformKind::Unsupported
            };
            let (cards, update) = update_settings_view(
                &identity,
                &self.settings_update_state,
                update_platform,
                self.settings_task_runtime.update_supported(),
                global_enabled,
            );
            (cards, Some(update))
        } else {
            let cards = if let Some(reason) = unavailable_reason.as_deref() {
                system_device_cards(state.category, reason)
            } else {
                settings_cards(
                    state,
                    config,
                    appearance,
                    global_enabled,
                    self.ascii_assets.theme_id(),
                    self.terminal_image_support,
                    &self.language_options(),
                    (self.platform_kind == PlatformKind::Linux).then_some(&state.system_time),
                )
            };
            (cards, None)
        };
        let picker = state.picker.as_ref().map(|picker| {
            let options = settings_picker_options(picker, &self.language_options());
            ui::SettingsPickerViewModel {
                editing: picker.editing.clone(),
                kind: picker.kind,
                title: picker_title(picker.kind),
                query: picker.query.clone(),
                selected_index: picker.selected_index.min(options.len().saturating_sub(1)),
                window_start: picker.window_start,
                searchable: true,
                options,
            }
        });
        let color_editor =
            state
                .color_editor
                .as_ref()
                .map(|editor| ui::SettingsColorEditorViewModel {
                    editing: editor.editing.clone(),
                    title: i18n::tr!("settings-custom-label", label = picker_label(editor.kind)),
                    value: editor.value.clone(),
                    error: editor
                        .error
                        .as_ref()
                        .map(i18n::LocalizedText::render_current),
                });
        let weather_location_editor = state.weather_location_editor.as_ref().map(|editor| {
            ui::SettingsWeatherLocationEditorViewModel {
                editing: editor.editing.clone(),
                value: editor.value.clone(),
                error: editor
                    .error
                    .as_ref()
                    .map(i18n::LocalizedText::render_current),
            }
        });
        let file_extensions_editor = state.file_extensions_editor.as_ref().map(|editor| {
            ui::SettingsFileExtensionsEditorViewModel {
                editing: editor.editing.clone(),
                value: editor.value.clone(),
                error: editor
                    .error
                    .as_ref()
                    .map(i18n::LocalizedText::render_current),
            }
        });
        let time_sync_server_editor = state.time_sync_server_editor.as_ref().map(|editor| {
            ui::SettingsTimeSyncServerEditorViewModel {
                editing: editor.editing.clone(),
                system_server_label: state.system_time.server_field.as_ref().and_then(|id| {
                    state
                        .system_time
                        .action("set_system_ntp_servers")
                        .and_then(|(_, action)| action.fields.iter().find(|field| &field.id == id))
                        .map(|field| field.label.clone())
                }),
                value: editor.value.clone(),
                error: editor
                    .error
                    .as_ref()
                    .map(i18n::LocalizedText::render_current),
                validating: editor.validating,
            }
        });
        Some(ui::SettingsViewModel {
            focus: state.focus,
            form_selected: state.form_selected,
            form_interaction: state.form_interaction.clone(),
            selected_application: state.application,
            selected_category: state.category,
            selected_field: state.selected_field,
            cards,
            status: if let Some(reason) = unavailable_reason.as_ref() {
                reason.clone()
            } else if state.category == ui::SettingsCategory::Update {
                if update
                    .as_ref()
                    .is_some_and(|update| update.panel == ui::SettingsUpdatePanel::Idle)
                    && self.settings_update_state.check_result.is_some()
                    && self.settings_update_state.error.is_none()
                {
                    String::new()
                } else {
                    self.settings_update_state.status.render_current()
                }
            } else {
                state.status.render_current()
            },
            locked_message: (!global_enabled
                && state.category != ui::SettingsCategory::Appearance
                && !state.category.is_system_device())
            .then_some(i18n::tr!("settings-locked")),
            scroll_offset: state.scroll_offset,
            animation_editor: state
                .animation_editor
                .as_ref()
                .map(|editor| editor.form.clone()),
            number_editor: state.number_editor.as_ref().map(|editor| {
                ui::SettingsNumberEditorViewModel {
                    field: editor.field,
                    title: editor.title.clone(),
                    input: editor.input.clone(),
                    minimum: editor.minimum,
                    maximum: editor.maximum,
                    error: editor
                        .error
                        .as_ref()
                        .map(i18n::LocalizedText::render_current),
                    selected_index: editor.selected_index,
                }
            }),
            picker,
            color_editor,
            weather_location_editor,
            file_extensions_editor,
            time_sync_server_editor,
            update,
        })
    }
}

pub(super) fn update_settings_view(
    identity: &app::update::BuildIdentity,
    update: &SettingsUpdateState,
    platform: PlatformKind,
    supported: bool,
    admin: bool,
) -> (Vec<ui::SettingsCardViewModel>, ui::SettingsUpdateViewModel) {
    use app::update::{UpdatePhase, UpdateRelation};
    use ui::{
        SettingsCardViewModel as Card, SettingsControlKind as Kind, SettingsField as Field,
        SettingsItemViewModel as Item, SettingsUpdatePanel as Panel,
    };
    let source = platform == PlatformKind::Windows
        || platform == PlatformKind::Linux && update.mode == storage::LinuxUpdateMode::Beta;
    let check = update.check_result.as_ref();
    let release = check.and_then(|check| check.release.as_ref());
    let available = update.build_available(identity);
    let working = update.busy || update.phase == Some(UpdatePhase::WaitingForRestart);
    let local_sha = identity
        .commit_sha
        .as_deref()
        .map(short_sha)
        .unwrap_or_else(|| i18n::tr!("settings-unknown"));
    let mut build = format!("{} ({local_sha})", identity.package_version);
    if !source
        && available
        && let Some((release, check)) = release.zip(check)
    {
        build = format!(
            "{build} → {} ({})",
            release.version,
            short_sha(&check.head_sha)
        );
    }
    let mut items = Vec::new();
    if matches!(platform, PlatformKind::Linux | PlatformKind::Windows) {
        let linux = platform == PlatformKind::Linux;
        items.push(
            Item::new(
                Field::UpdateMode,
                i18n::tr!("settings-update-mode"),
                if !linux {
                    i18n::tr!("settings-update-mode-source")
                } else if source {
                    i18n::tr!("settings-update-mode-beta")
                } else {
                    i18n::tr!("settings-update-mode-release")
                },
                i18n::tr!("settings-update-mode-help"),
                if linux { Kind::Action } else { Kind::ReadOnly },
            )
            .enabled(!linux || admin && !working),
        );
    }
    items.push(Item::new(
        Field::InstalledVersion,
        i18n::tr!("settings-current-build"),
        build,
        "",
        Kind::ReadOnly,
    ));
    if source {
        items.push(Item::new(
            Field::RemoteVersion,
            i18n::tr!("settings-remote-build"),
            check
                .map(|check| format!("{} @ {}", check.default_branch, short_sha(&check.head_sha)))
                .unwrap_or_else(|| i18n::tr!("settings-not-checked")),
            "",
            Kind::ReadOnly,
        ));
    }

    let mut activity = update.activity.clone().unwrap_or_default();
    activity.show_build_details = source;
    let panel = if source {
        let tree = check.and_then(|check| check.git_tree.as_ref());
        let message = match tree {
            Some(Err(reason)) => {
                i18n::tr!("settings-git-tree-unavailable", reason = reason.clone())
            }
            _ => update.status.render_current(),
        };
        Panel::Source {
            rows: tree
                .and_then(|tree| tree.as_ref().ok())
                .cloned()
                .unwrap_or_default(),
            message,
            selected: update.git_selected,
            offset: update.git_offset,
            selectable: !working,
            activity,
        }
    } else if available && release.is_some() {
        Panel::Release {
            notes: check
                .and_then(|check| check.commits.first())
                .map(|notes| notes.message.clone())
                .unwrap_or_default(),
            activity,
        }
    } else {
        Panel::Idle
    };
    let action = (!matches!(panel, Panel::Idle)).then(|| {
        Item::new(
            Field::StartUpdate,
            if source {
                i18n::tr!("settings-build-selected")
            } else {
                i18n::tr!("settings-start-update")
            },
            "",
            "",
            Kind::Action,
        )
        .enabled(supported && admin && available && !working)
    });
    let confirmation = update
        .confirmed_sha
        .as_ref()
        .zip(check)
        .map(|(sha, check)| {
            let replacement = identity.dirty
                || !matches!(check.relation, UpdateRelation::Behind { .. })
                || sha != &check.head_sha;
            ui::SettingsUpdateConfirmationViewModel {
                title: if replacement {
                    i18n::tr!("settings-replace-github")
                } else {
                    i18n::tr!("settings-install-update")
                },
                body: if release.is_some() {
                    i18n::tr!("settings-release-install-body")
                } else {
                    i18n::tr!("settings-build-selected-body", sha = sha.clone())
                },
                confirm_label: i18n::tr!("settings-confirm"),
                confirm_selected: update.confirm_selected,
            }
        });
    (
        vec![Card::new("", items)],
        ui::SettingsUpdateViewModel {
            panel,
            action,
            confirmation,
        },
    )
}

pub(super) fn short_sha(value: &str) -> String {
    value.chars().take(7).collect()
}

pub(super) fn checked_update_label(
    result: &app::update::UpdateCheckResult,
) -> i18n::LocalizedMessage {
    if result.release.is_some() {
        match result.relation {
            app::update::UpdateRelation::Identical => i18n::msg!("settings-up-to-date"),
            app::update::UpdateRelation::Behind { .. } => i18n::msg!("settings-release-available"),
            _ => i18n::msg!("settings-release-different"),
        }
    } else {
        update_relation_label(&result.relation)
    }
}

pub(super) fn update_relation_label(
    relation: &app::update::UpdateRelation,
) -> i18n::LocalizedMessage {
    match relation {
        app::update::UpdateRelation::Identical => i18n::msg!("settings-up-to-date"),
        app::update::UpdateRelation::Behind { remote_ahead } => {
            i18n::msg!("settings-update-behind", count = *remote_ahead)
        }
        app::update::UpdateRelation::Ahead { local_ahead } => {
            i18n::msg!("settings-update-ahead", count = *local_ahead)
        }
        app::update::UpdateRelation::Diverged {
            remote_ahead,
            local_ahead,
        } => i18n::msg!(
            "settings-update-diverged",
            remote = *remote_ahead,
            local = *local_ahead
        ),
        app::update::UpdateRelation::Unknown => i18n::msg!("settings-unknown-commit"),
    }
}

pub(in crate::session) fn settings_cards(
    state: &SettingsState,
    config: &storage::StorageConfig,
    appearance: &storage::AppearanceConfig,
    global_enabled: bool,
    asset_theme_id: &str,
    image_icons_supported: bool,
    languages: &[app::SetupLanguageOption],
    native_time: Option<&SystemTimeSettings>,
) -> Vec<ui::SettingsCardViewModel> {
    use ui::{
        SettingsCardViewModel as Card, SettingsControlKind as Kind, SettingsField as Field,
        SettingsItemViewModel as Item,
    };
    let toggle = |field, label, value: bool, description, enabled| {
        Item::new(
            field,
            label,
            if value {
                i18n::tr!("settings-on")
            } else {
                i18n::tr!("settings-off")
            },
            description,
            Kind::Toggle,
        )
        .checked(value)
        .enabled(enabled)
    };
    let reset = |enabled| {
        Item::new(
            Field::RestoreDefaults,
            i18n::tr!("settings-restore-defaults"),
            i18n::tr!("settings-restore-defaults"),
            i18n::tr!("settings-restore-help"),
            Kind::Action,
        )
        .enabled(enabled)
    };
    match state.category {
        ui::SettingsCategory::Appearance => vec![
            Card::new(
                i18n::tr!("settings-theme"),
                vec![
                    Item::new(
                        Field::Theme,
                        i18n::tr!("settings-theme"),
                        if asset_theme_id == ui::DEFAULT_THEME_ID {
                            match (appearance.icon_display_mode, image_icons_supported) {
                                (storage::IconDisplayMode::Image, true) => {
                                    i18n::tr!("settings-default-image-icons")
                                }
                                _ => i18n::tr!("settings-default-ascii-icons"),
                            }
                        } else {
                            asset_theme_id.to_string()
                        },
                        if asset_theme_id == ui::DEFAULT_THEME_ID {
                            i18n::tr!("settings-icon-options-help")
                        } else {
                            i18n::tr!("settings-icon-switch-help")
                        },
                        Kind::Picker,
                    )
                    .enabled(asset_theme_id == ui::DEFAULT_THEME_ID),
                ],
            ),
            Card::new(
                i18n::tr!("settings-visual-style"),
                vec![
                    Item::new(
                        Field::BorderShape,
                        i18n::tr!("settings-border-shape"),
                        match appearance.border_shape {
                            storage::BorderShape::Rounded => i18n::tr!("settings-rounded"),
                            storage::BorderShape::Square => i18n::tr!("settings-square"),
                        },
                        i18n::tr!("settings-border-shape-help"),
                        Kind::Cycle,
                    ),
                    Item::new(
                        Field::BorderColor,
                        i18n::tr!("settings-border-color"),
                        settings_color_label(appearance.border_color),
                        i18n::tr!("settings-border-color-help"),
                        Kind::Palette,
                    ),
                    Item::new(
                        Field::AccentColor,
                        i18n::tr!("settings-accent-color"),
                        settings_color_label(appearance.accent_color),
                        i18n::tr!("settings-accent-color-help"),
                        Kind::Palette,
                    ),
                    Item::new(
                        Field::RecheckCellIcons,
                        i18n::tr!("settings-cell-icons"),
                        i18n::tr!("settings-cell-icons-recheck"),
                        i18n::tr!("settings-cell-icons-help"),
                        Kind::Action,
                    ),
                ],
            ),
            Card::new(
                i18n::tr!("settings-animation"),
                super::animation::items(appearance),
            ),
            Card::new(i18n::tr!("settings-reset"), vec![reset(true)]),
        ],
        ui::SettingsCategory::RegionTime => vec![
            Card::new(
                i18n::tr!("settings-language-timezone"),
                vec![
                    Item::new(
                        Field::Language,
                        i18n::tr!("settings-language"),
                        language_label(&config.language, languages),
                        i18n::tr!("settings-language-help"),
                        Kind::Picker,
                    )
                    .enabled(global_enabled),
                    if let Some(time) = native_time {
                        time.configure_item(
                            Item::new(
                                Field::Timezone,
                                i18n::tr!("settings-city-timezone"),
                                time.timezone()
                                    .map(timezone_label)
                                    .unwrap_or_else(|| i18n::tr!("settings-operating-system")),
                                i18n::tr!("settings-system-timezone-help"),
                                Kind::Picker,
                            )
                            .enabled(global_enabled),
                            "set_system_timezone",
                        )
                    } else {
                        Item::new(
                            Field::Timezone,
                            i18n::tr!("settings-city-timezone"),
                            timezone_label(&config.timezone),
                            i18n::tr!("settings-timezone-help"),
                            Kind::Picker,
                        )
                        .enabled(global_enabled)
                    },
                    Item::new(
                        Field::WeatherLocation,
                        i18n::tr!("settings-weather-location"),
                        config
                            .weather_location
                            .clone()
                            .unwrap_or_else(|| i18n::tr!("settings-same-timezone")),
                        i18n::tr!("settings-weather-help"),
                        Kind::Picker,
                    )
                    .enabled(global_enabled),
                ],
            ),
            Card::new(
                i18n::tr!("settings-time-sync"),
                if let Some(time) = native_time {
                    let automatic = time
                        .action("set_system_ntp")
                        .and_then(|(_, action)| {
                            action.fields.iter().find(|field| field.id == "enabled")
                        })
                        .map(|field| {
                            i18n::tr!(if field.value == "true" {
                                "settings-on"
                            } else {
                                "settings-off"
                            })
                        })
                        .unwrap_or_else(|| {
                            i18n::tr!(if time.busy()
                                || (time.snapshot.is_none() && time.error.is_none())
                            {
                                "management-loading"
                            } else {
                                "settings-device-unavailable"
                            })
                        });
                    let servers = time
                        .action("set_system_ntp_servers")
                        .map(|(_, action)| {
                            action
                                .fields
                                .iter()
                                .filter(|field| {
                                    field.id.starts_with("source_") && !field.value.is_empty()
                                })
                                .map(|field| field.value.clone())
                                .collect::<Vec<_>>()
                                .join(", ")
                        })
                        .filter(|value| !value.is_empty())
                        .unwrap_or_else(|| i18n::tr!("settings-automatic-servers"));
                    vec![
                        time.configure_item(
                            Item::new(
                                Field::TimeSyncSource,
                                i18n::tr!("system-settings-automatic"),
                                automatic,
                                i18n::tr!("settings-system-time-help"),
                                Kind::Toggle,
                            )
                            .checked(
                                time.action("set_system_ntp")
                                    .and_then(|(_, action)| {
                                        action.fields.iter().find(|field| field.id == "enabled")
                                    })
                                    .is_some_and(|field| field.value == "true"),
                            )
                            .enabled(global_enabled),
                            "set_system_ntp",
                        ),
                        time.configure_item(
                            Item::new(
                                Field::TimeSyncServer,
                                i18n::tr!("settings-sync-server"),
                                servers,
                                i18n::tr!("settings-system-time-server-help"),
                                Kind::Picker,
                            )
                            .enabled(global_enabled),
                            if time.action("set_system_ntp_servers").is_some() {
                                "set_system_ntp_servers"
                            } else {
                                "inspect_system_settings"
                            },
                        ),
                        Item::new(
                            Field::RefreshSystemTime,
                            i18n::tr!("settings-system-time-refresh"),
                            i18n::tr!("management-control-refresh"),
                            i18n::tr!("settings-system-time-refresh-help"),
                            Kind::Action,
                        )
                        .enabled(!time.busy()),
                    ]
                } else {
                    vec![
                        Item::new(
                            Field::TimeSyncSource,
                            i18n::tr!("settings-time-source"),
                            time_sync_source_label(config.time_sync.source),
                            i18n::tr!("settings-time-source-help"),
                            Kind::Cycle,
                        )
                        .enabled(global_enabled),
                        Item::new(
                            Field::TimeSyncServer,
                            i18n::tr!("settings-sync-server"),
                            config
                                .time_sync
                                .server_url
                                .clone()
                                .unwrap_or_else(|| i18n::tr!("settings-automatic-servers")),
                            i18n::tr!("settings-time-server-help"),
                            Kind::Picker,
                        )
                        .enabled(
                            global_enabled
                                && config.time_sync.source
                                    == storage::TimeSyncSource::NetworkServer,
                        ),
                    ]
                },
            ),
            Card::new(i18n::tr!("settings-reset"), vec![reset(global_enabled)]),
        ],
        ui::SettingsCategory::System => vec![
            Card::new(
                "AutoAdmin (AA)",
                vec![
                    Item::new(
                        Field::AutoAdmin,
                        i18n::tr!("aa-policy"),
                        crate::session::controller::auto_admin::policy_label(config.auto_admin),
                        i18n::tr!("aa-policy-help"),
                        Kind::Picker,
                    )
                    .enabled(global_enabled),
                ],
            ),
            Card::new(
                i18n::tr!("ui-control-settings-title"),
                vec![toggle(
                    Field::AutomationEnabled,
                    i18n::tr!("ui-control-settings-enable"),
                    config.automation.enabled,
                    i18n::tr!("ui-control-settings-help"),
                    global_enabled,
                )],
            ),
            Card::new(
                i18n::tr!("settings-screen-timeout"),
                vec![
                    Item::new(
                        Field::ScreenTimeout,
                        i18n::tr!("settings-screen-timeout"),
                        match config.screen_timeout_seconds {
                            0 => i18n::tr!("settings-screen-timeout-never"),
                            seconds if seconds % 60 == 0 => {
                                i18n::tr!("settings-screen-timeout-minutes", value = seconds / 60)
                            }
                            seconds => {
                                i18n::tr!("settings-screen-timeout-seconds", value = seconds)
                            }
                        },
                        i18n::tr!("settings-screen-timeout-help"),
                        Kind::Picker,
                    )
                    .enabled(global_enabled),
                ],
            ),
            Card::new(
                i18n::tr!("settings-storage-pressure"),
                vec![
                    Item::new(
                        Field::SystemLowAvailable,
                        i18n::tr!("settings-low-available"),
                        i18n::tr!(
                            "settings-value-gib",
                            value = config.system_status.low_available_gib
                        ),
                        i18n::tr!("settings-low-available-help"),
                        Kind::Stepper,
                    )
                    .enabled(global_enabled),
                    Item::new(
                        Field::SystemLowPercentage,
                        i18n::tr!("settings-low-percentage"),
                        i18n::tr!(
                            "settings-value-percent",
                            value = config.system_status.low_percentage
                        ),
                        i18n::tr!("settings-low-percentage-help"),
                        Kind::Stepper,
                    )
                    .enabled(global_enabled),
                    Item::new(
                        Field::SystemCriticalAvailable,
                        i18n::tr!("settings-critical-available"),
                        i18n::tr!(
                            "settings-value-gib",
                            value = config.system_status.critical_available_gib
                        ),
                        i18n::tr!("settings-critical-available-help"),
                        Kind::Stepper,
                    )
                    .enabled(global_enabled),
                    Item::new(
                        Field::SystemCriticalPercentage,
                        i18n::tr!("settings-critical-percentage"),
                        i18n::tr!(
                            "settings-value-percent",
                            value = config.system_status.critical_percentage
                        ),
                        i18n::tr!("settings-critical-percentage-help"),
                        Kind::Stepper,
                    )
                    .enabled(global_enabled),
                ],
            ),
            Card::new(i18n::tr!("settings-top-bar"), {
                let top_bar = config.top_bar;
                let checkbox = |field, label, checked| {
                    Item::new(
                        field,
                        label,
                        if checked { "[x]" } else { "[ ]" },
                        "",
                        Kind::Toggle,
                    )
                    .checked(checked)
                    .enabled(global_enabled)
                };
                vec![
                    Item::new(
                        Field::TopBarStatusAlignment,
                        i18n::tr!("settings-top-bar-status-alignment"),
                        status_alignment_label(top_bar.status_alignment),
                        "",
                        Kind::Picker,
                    )
                    .enabled(global_enabled),
                    checkbox(
                        Field::TopBarHelp,
                        i18n::tr!("settings-top-bar-show-help"),
                        top_bar.show_help,
                    ),
                    checkbox(
                        Field::TopBarTime,
                        i18n::tr!("settings-top-bar-show-time"),
                        top_bar.show_time,
                    ),
                    checkbox(
                        Field::TopBarEsc,
                        i18n::tr!("settings-top-bar-show-esc"),
                        top_bar.show_esc,
                    ),
                ]
            }),
            Card::new(i18n::tr!("settings-reset"), vec![reset(global_enabled)]),
        ],
        ui::SettingsCategory::Applications
            if state.application == ui::SettingsApplication::FileExplorer =>
        {
            vec![
                Card::new(
                    i18n::tr!("settings-display"),
                    vec![
                        toggle(
                            Field::ShowHidden,
                            i18n::tr!("settings-show-hidden"),
                            config.explorer.show_hidden,
                            i18n::tr!("settings-show-hidden-help"),
                            global_enabled,
                        ),
                        toggle(
                            Field::ShowSystem,
                            i18n::tr!("settings-show-system"),
                            config.explorer.show_system,
                            i18n::tr!("settings-show-system-help"),
                            global_enabled,
                        ),
                        toggle(
                            Field::ShowExtensions,
                            i18n::tr!("settings-show-extensions"),
                            config.explorer.show_extensions,
                            i18n::tr!("settings-show-extensions-help"),
                            global_enabled,
                        ),
                        toggle(
                            Field::FoldersFirst,
                            i18n::tr!("settings-folders-first"),
                            config.explorer.folders_first,
                            i18n::tr!("settings-folders-first-help"),
                            global_enabled,
                        ),
                        toggle(
                            Field::ShowSidebar,
                            i18n::tr!("settings-show-quick-access"),
                            config.explorer.show_sidebar,
                            i18n::tr!("settings-quick-access-help"),
                            global_enabled,
                        ),
                    ],
                ),
                Card::new(
                    i18n::tr!("settings-sorting-format"),
                    vec![
                        toggle(
                            Field::CaseSensitiveSort,
                            i18n::tr!("settings-case-sensitive"),
                            config.explorer.case_sensitive_sort,
                            i18n::tr!("settings-case-sensitive-help"),
                            global_enabled,
                        ),
                        Item::new(
                            Field::SizeFormat,
                            i18n::tr!("settings-size-format"),
                            size_format_label(config.explorer.size_format),
                            i18n::tr!("settings-size-format-help"),
                            Kind::Cycle,
                        )
                        .enabled(global_enabled),
                        Item::new(
                            Field::DateZone,
                            i18n::tr!("settings-date-timezone"),
                            date_zone_label(config.explorer.date_zone),
                            i18n::tr!("settings-date-timezone-help"),
                            Kind::Cycle,
                        )
                        .enabled(global_enabled),
                        Item::new(
                            Field::SortField,
                            i18n::tr!("settings-default-sort-field"),
                            sort_field_label(config.explorer.sort_field),
                            i18n::tr!("settings-sort-field-help"),
                            Kind::Cycle,
                        )
                        .enabled(global_enabled),
                        Item::new(
                            Field::SortDirection,
                            i18n::tr!("settings-default-direction"),
                            sort_direction_label(config.explorer.sort_direction),
                            i18n::tr!("settings-sort-direction-help"),
                            Kind::Cycle,
                        )
                        .enabled(global_enabled),
                    ],
                ),
                Card::new(
                    i18n::tr!("settings-safety"),
                    vec![
                        toggle(
                            Field::ConfirmDelete,
                            i18n::tr!("settings-confirm-delete"),
                            config.explorer.confirm_delete,
                            i18n::tr!("settings-confirm-delete-help"),
                            global_enabled,
                        ),
                        toggle(
                            Field::ConfirmNameConflicts,
                            i18n::tr!("settings-confirm-conflicts"),
                            config.explorer.confirm_name_conflicts,
                            i18n::tr!("settings-confirm-conflicts-help"),
                            global_enabled,
                        ),
                    ],
                ),
                Card::new(i18n::tr!("settings-reset"), vec![reset(global_enabled)]),
            ]
        }
        ui::SettingsCategory::Applications
            if state.application == ui::SettingsApplication::CommandLine =>
        {
            vec![
                Card::new(
                    i18n::tr!("settings-command-line-system-commands"),
                    vec![
                        Item::new(
                            Field::CommandLineShell,
                            i18n::tr!("settings-command-line-shell"),
                            command_line_shell_label(
                                config.command_line.shell.for_current_platform(),
                            ),
                            i18n::tr!("settings-command-line-shell-help"),
                            Kind::Picker,
                        )
                        .enabled(global_enabled),
                    ],
                ),
                Card::new(i18n::tr!("settings-reset"), vec![reset(global_enabled)]),
            ]
        }
        ui::SettingsCategory::Applications => vec![
            Card::new(
                i18n::tr!("settings-explorer-opening"),
                vec![
                    Item::new(
                        Field::ExplorerOpenExtensions,
                        i18n::tr!("settings-open-editor"),
                        editor_extensions_summary(&config.editor.explorer_open_extensions),
                        i18n::tr!("settings-open-editor-help"),
                        Kind::Picker,
                    )
                    .enabled(global_enabled),
                ],
            ),
            Card::new(
                i18n::tr!("settings-cursor-acceleration"),
                vec![
                    toggle(
                        Field::CursorAcceleration,
                        i18n::tr!("settings-cursor-acceleration"),
                        config.editor.cursor_acceleration_enabled,
                        i18n::tr!("settings-cursor-acceleration-help"),
                        global_enabled,
                    ),
                    Item::new(
                        Field::CursorDelay,
                        i18n::tr!("settings-start-delay"),
                        i18n::tr!(
                            "settings-value-ms",
                            value = config.editor.cursor_acceleration_delay_ms
                        ),
                        i18n::tr!("settings-cursor-delay-help"),
                        Kind::Stepper,
                    )
                    .enabled(global_enabled),
                    Item::new(
                        Field::CursorRamp,
                        i18n::tr!("settings-ramp-maximum"),
                        i18n::tr!(
                            "settings-value-ms",
                            value = config.editor.cursor_acceleration_ramp_ms
                        ),
                        i18n::tr!("settings-cursor-ramp-help"),
                        Kind::Stepper,
                    )
                    .enabled(global_enabled),
                    Item::new(
                        Field::CursorHorizontalStep,
                        i18n::tr!("settings-horizontal-maximum"),
                        i18n::tr!(
                            "settings-value-cells",
                            value = config.editor.cursor_horizontal_max_step
                        ),
                        i18n::tr!("settings-horizontal-help"),
                        Kind::Stepper,
                    )
                    .enabled(global_enabled),
                    Item::new(
                        Field::CursorVerticalStep,
                        i18n::tr!("settings-vertical-maximum"),
                        i18n::tr!(
                            "settings-value-lines",
                            value = config.editor.cursor_vertical_max_step
                        ),
                        i18n::tr!("settings-vertical-help"),
                        Kind::Stepper,
                    )
                    .enabled(global_enabled),
                ],
            ),
            Card::new(i18n::tr!("settings-reset"), vec![reset(global_enabled)]),
        ],
        ui::SettingsCategory::Update
        | ui::SettingsCategory::Sound
        | ui::SettingsCategory::Display
        | ui::SettingsCategory::Wifi
        | ui::SettingsCategory::Bluetooth => Vec::new(),
    }
}

pub(in crate::session) fn settings_picker_options(
    picker: &SettingsPickerState,
    languages: &[app::SetupLanguageOption],
) -> Vec<ui::SettingsPickerOptionViewModel> {
    let options: Vec<ui::SettingsPickerOptionViewModel> = match picker.kind {
        ui::SettingsPickerKind::AutoAdmin => AUTO_ADMIN_POLICIES
            .into_iter()
            .map(|policy| {
                ui::SettingsPickerOptionViewModel::new(
                    crate::session::controller::auto_admin::policy_label(policy),
                    "",
                )
            })
            .collect(),
        ui::SettingsPickerKind::ScreenTimeout => SCREEN_TIMEOUT_OPTIONS
            .iter()
            .map(|seconds| {
                let label = match *seconds {
                    0 => i18n::tr!("settings-screen-timeout-never"),
                    seconds if seconds % 60 == 0 => {
                        i18n::tr!("settings-screen-timeout-minutes", value = seconds / 60)
                    }
                    seconds => i18n::tr!("settings-screen-timeout-seconds", value = seconds),
                };
                ui::SettingsPickerOptionViewModel::new(label, "")
            })
            .collect(),
        ui::SettingsPickerKind::TopBarStatusAlignment => TOP_BAR_ALIGNMENTS
            .into_iter()
            .map(|alignment| {
                ui::SettingsPickerOptionViewModel::new(status_alignment_label(alignment), "")
            })
            .collect(),
        ui::SettingsPickerKind::CommandLineShell => SystemCommandShell::choices()
            .iter()
            .map(|&shell| {
                ui::SettingsPickerOptionViewModel::new(
                    command_line_shell_label(shell),
                    shell.program().unwrap_or_default(),
                )
            })
            .collect(),
        ui::SettingsPickerKind::SystemTimeServers => picker
            .options
            .iter()
            .map(|(label, value)| {
                ui::SettingsPickerOptionViewModel::new(label.clone(), value.clone())
            })
            .collect(),
        ui::SettingsPickerKind::Theme => vec![ui::SettingsPickerOptionViewModel::new(
            i18n::tr!("settings-default-theme"),
            i18n::tr!("settings-built-in-theme"),
        )],
        ui::SettingsPickerKind::DefaultThemeIcons => vec![
            ui::SettingsPickerOptionViewModel::new(
                i18n::tr!("settings-ascii-icons"),
                i18n::tr!("settings-ascii-icons-help"),
            ),
            ui::SettingsPickerOptionViewModel::new(
                i18n::tr!("settings-image-icons"),
                i18n::tr!("settings-image-icons-help"),
            )
            .enabled(picker.image_icons_supported),
        ],
        ui::SettingsPickerKind::Language => languages
            .iter()
            .cloned()
            .map(|option| ui::SettingsPickerOptionViewModel::new(option.label, option.code))
            .collect(),
        ui::SettingsPickerKind::Timezone => app::setup_timezone_options()
            .into_iter()
            .map(|option| {
                ui::SettingsPickerOptionViewModel::new(
                    option.localized_label().render_current(),
                    option.localized_description().render_current(),
                )
                .with_keywords([option.label.clone(), option.description.clone()])
                .timezone(option.id, option.longitude, option.latitude)
            })
            .collect(),
        ui::SettingsPickerKind::BorderColor | ui::SettingsPickerKind::AccentColor => {
            let mut options = ui::setup_standard_color_options()
                .iter()
                .map(|option| {
                    ui::SettingsPickerOptionViewModel::new(option.label.clone(), option.value)
                })
                .collect::<Vec<_>>();
            options.push(ui::SettingsPickerOptionViewModel::new(
                i18n::tr!("settings-custom-color"),
                "#RRGGBB",
            ));
            options
        }
    };
    options
        .into_iter()
        .enumerate()
        .filter_map(|(index, mut option)| {
            option.source_index = Some(index);
            ui::components::selection_matches(
                &picker.query,
                [
                    option.label.as_str(),
                    option.detail.as_str(),
                    option.timezone_id.as_deref().unwrap_or(""),
                ]
                .into_iter()
                .chain(option.keywords.iter().map(String::as_str)),
            )
            .then_some(option)
        })
        .collect()
}

pub(in crate::session) fn color_picker_initial_index(color: storage::BorderColor) -> usize {
    ui::setup_standard_color_options()
        .iter()
        .position(|option| option.value == color.to_string())
        .unwrap_or_else(|| ui::setup_standard_color_options().len())
}

pub(in crate::session) fn settings_picker_visible_rows(terminal_height: u16) -> usize {
    usize::from(terminal_height.saturating_sub(10).clamp(4, 18))
}

pub(in crate::session) fn picker_title(kind: ui::SettingsPickerKind) -> String {
    match kind {
        ui::SettingsPickerKind::AutoAdmin => i18n::tr!("aa-policy"),
        ui::SettingsPickerKind::ScreenTimeout => i18n::tr!("settings-screen-timeout"),
        ui::SettingsPickerKind::TopBarStatusAlignment => {
            i18n::tr!("settings-top-bar-status-alignment")
        }
        ui::SettingsPickerKind::CommandLineShell => i18n::tr!("settings-command-line-shell"),
        ui::SettingsPickerKind::SystemTimeServers => i18n::tr!("system-settings-edit-servers"),
        ui::SettingsPickerKind::Theme => i18n::tr!("settings-choose-theme"),
        ui::SettingsPickerKind::DefaultThemeIcons => i18n::tr!("settings-default-theme"),
        ui::SettingsPickerKind::Language => i18n::tr!("settings-choose-language"),
        ui::SettingsPickerKind::Timezone => i18n::tr!("settings-choose-timezone"),
        ui::SettingsPickerKind::BorderColor => i18n::tr!("settings-choose-border-color"),
        ui::SettingsPickerKind::AccentColor => i18n::tr!("settings-choose-accent-color"),
    }
}

pub(in crate::session) fn picker_label(kind: ui::SettingsPickerKind) -> String {
    match kind {
        ui::SettingsPickerKind::AutoAdmin => i18n::tr!("aa-policy"),
        ui::SettingsPickerKind::ScreenTimeout => i18n::tr!("settings-screen-timeout"),
        ui::SettingsPickerKind::TopBarStatusAlignment => {
            i18n::tr!("settings-top-bar-status-alignment")
        }
        ui::SettingsPickerKind::CommandLineShell => i18n::tr!("settings-command-line-shell"),
        ui::SettingsPickerKind::SystemTimeServers => i18n::tr!("system-settings-servers"),
        ui::SettingsPickerKind::Theme => i18n::tr!("settings-theme"),
        ui::SettingsPickerKind::DefaultThemeIcons => i18n::tr!("settings-default-icon-mode"),
        ui::SettingsPickerKind::BorderColor => i18n::tr!("settings-border-color"),
        ui::SettingsPickerKind::AccentColor => i18n::tr!("settings-accent-color"),
        ui::SettingsPickerKind::Language => i18n::tr!("settings-language"),
        ui::SettingsPickerKind::Timezone => i18n::tr!("settings-timezone"),
    }
}

pub(in crate::session) fn language_label(
    code: &str,
    languages: &[app::SetupLanguageOption],
) -> String {
    languages
        .iter()
        .find(|option| option.code == code)
        .map(|option| format!("{} ({})", option.label, option.code))
        .unwrap_or_else(|| code.to_string())
}

pub(super) fn settings_color_label(color: storage::BorderColor) -> String {
    let value = color.to_string();
    ui::setup_standard_color_options()
        .into_iter()
        .find(|option| option.value == value)
        .map(|option| option.label)
        .unwrap_or(value)
}

pub(in crate::session) fn timezone_label(id: &str) -> String {
    app::setup_timezone_options()
        .into_iter()
        .find(|option| option.id == id)
        .map(|option| {
            format!(
                "{} ({})",
                option.localized_label().render_current(),
                option.id
            )
        })
        .unwrap_or_else(|| id.to_string())
}

pub(in crate::session) fn time_sync_source_label(source: storage::TimeSyncSource) -> String {
    match source {
        storage::TimeSyncSource::NetworkServer => i18n::tr!("settings-network-server"),
        storage::TimeSyncSource::OperatingSystem => i18n::tr!("settings-operating-system"),
    }
}

fn command_line_shell_label(shell: SystemCommandShell) -> String {
    match shell {
        SystemCommandShell::Default => i18n::tr!("settings-command-line-shell-default"),
        SystemCommandShell::Cmd => i18n::tr!("settings-command-line-shell-cmd"),
        SystemCommandShell::WindowsPowerShell => {
            i18n::tr!("settings-command-line-shell-powershell")
        }
        SystemCommandShell::Pwsh => i18n::tr!("settings-command-line-shell-pwsh"),
        SystemCommandShell::Sh => i18n::tr!("settings-command-line-shell-sh"),
        SystemCommandShell::Bash => i18n::tr!("settings-command-line-shell-bash"),
        SystemCommandShell::Zsh => i18n::tr!("settings-command-line-shell-zsh"),
        SystemCommandShell::Fish => i18n::tr!("settings-command-line-shell-fish"),
    }
}

fn status_alignment_label(alignment: storage::StatusTextAlignment) -> String {
    match alignment {
        storage::StatusTextAlignment::Left => i18n::tr!("settings-top-bar-align-left"),
        storage::StatusTextAlignment::Center => i18n::tr!("settings-top-bar-align-center"),
        storage::StatusTextAlignment::Right => i18n::tr!("settings-top-bar-align-right"),
    }
}

pub(in crate::session) fn cycle_explorer_sort_field(
    value: storage::ExplorerSortField,
    delta: isize,
) -> storage::ExplorerSortField {
    let values = [
        storage::ExplorerSortField::Name,
        storage::ExplorerSortField::Type,
        storage::ExplorerSortField::Size,
        storage::ExplorerSortField::Modified,
    ];
    let index = values.iter().position(|item| *item == value).unwrap_or(0) as isize;
    values[(index + delta).rem_euclid(values.len() as isize) as usize]
}

pub(super) fn settings_saved_field(field: ui::SettingsField) -> i18n::LocalizedMessage {
    i18n::msg!("settings-saved-field", field = format!("{field:?}"))
}

pub(super) fn settings_saved_picker(kind: ui::SettingsPickerKind) -> i18n::LocalizedMessage {
    let field = match kind {
        ui::SettingsPickerKind::BorderColor => ui::SettingsField::BorderColor,
        ui::SettingsPickerKind::AccentColor => ui::SettingsField::AccentColor,
        _ => unreachable!("only color pickers save appearance through this helper"),
    };
    settings_saved_field(field)
}

pub(in crate::session) fn is_weather_location_character(character: char) -> bool {
    character.is_ascii_alphanumeric()
        || matches!(character, ' ' | ',' | '.' | '-' | '\'' | '/' | '(' | ')')
}

pub(in crate::session) fn is_editor_extension_input_character(character: char) -> bool {
    character.is_ascii_alphanumeric()
        || character.is_ascii_whitespace()
        || matches!(character, '.' | ',' | ';' | '_' | '-' | '+')
}

pub(in crate::session) fn parse_editor_explorer_open_extensions(
    value: &str,
) -> Result<Vec<String>, i18n::LocalizedText> {
    let mut extensions = Vec::new();
    for raw in value.split(|character: char| {
        character == ',' || character == ';' || character.is_ascii_whitespace()
    }) {
        if raw.is_empty() {
            continue;
        }
        let Some(extension) = storage::normalize_editor_explorer_open_extension(raw) else {
            return Err(i18n::msg!("settings-invalid-suffix", suffix = format!("{raw:?}")).into());
        };
        if extensions.contains(&extension) {
            continue;
        }
        if extensions.len() >= storage::MAX_EDITOR_EXPLORER_OPEN_EXTENSIONS {
            return Err(i18n::msg!(
                "settings-suffix-count-limit",
                limit = storage::MAX_EDITOR_EXPLORER_OPEN_EXTENSIONS
            )
            .into());
        }
        extensions.push(extension);
    }
    Ok(extensions)
}

pub(super) fn settings_scroll_offset(current: u16, delta: i16, maximum: u16) -> u16 {
    let next = if delta < 0 {
        current.saturating_sub(delta.unsigned_abs())
    } else {
        current.saturating_add(delta as u16)
    };
    next.min(maximum)
}

pub(in crate::session) fn format_editor_explorer_open_extensions(extensions: &[String]) -> String {
    extensions
        .iter()
        .map(|extension| format!(".{extension}"))
        .collect::<Vec<_>>()
        .join(", ")
}

pub(in crate::session) fn editor_extensions_summary(extensions: &[String]) -> String {
    if extensions.is_empty() {
        return i18n::tr!("settings-system-default");
    }
    if extensions.len() <= 4 {
        return format_editor_explorer_open_extensions(extensions);
    }
    i18n::tr!(
        "settings-more-suffixes",
        suffixes = format_editor_explorer_open_extensions(&extensions[..3]),
        count = extensions.len() - 3
    )
}

pub(in crate::session) fn size_format_label(value: storage::ExplorerSizeFormat) -> String {
    match value {
        storage::ExplorerSizeFormat::HumanBinary => i18n::tr!("settings-human-binary"),
        storage::ExplorerSizeFormat::Bytes => i18n::tr!("settings-bytes"),
    }
}

pub(in crate::session) fn date_zone_label(value: storage::ExplorerDateZone) -> String {
    match value {
        storage::ExplorerDateZone::ConfiguredTimezone => i18n::tr!("settings-configured-timezone"),
        storage::ExplorerDateZone::Utc => i18n::tr!("settings-utc"),
    }
}

pub(in crate::session) fn sort_field_label(value: storage::ExplorerSortField) -> String {
    match value {
        storage::ExplorerSortField::Name => i18n::tr!("settings-name"),
        storage::ExplorerSortField::Type => i18n::tr!("settings-type"),
        storage::ExplorerSortField::Size => i18n::tr!("settings-size"),
        storage::ExplorerSortField::Modified => i18n::tr!("settings-modified"),
    }
}

pub(in crate::session) fn sort_direction_label(value: storage::ExplorerSortDirection) -> String {
    match value {
        storage::ExplorerSortDirection::Ascending => i18n::tr!("settings-ascending"),
        storage::ExplorerSortDirection::Descending => i18n::tr!("settings-descending"),
    }
}

#[cfg(test)]
#[path = "../../../../tests/unit/session/controller/settings/picker_tests.rs"]
mod picker_tests;
