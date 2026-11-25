use super::overlays::{ResolvedExplorerOverlay, ShellOverlayCategory};
use super::*;

mod clock;
mod editor;
mod explorer;
mod explorer_dialogs;
mod launcher;
mod navigation;
mod overlays;
mod pdf;
mod status_dashboard;
mod status_monitoring;

struct SystemStatusTempGuard(PathBuf);

impl Drop for SystemStatusTempGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn set_test_auth_role(state: &mut ShellSession, role: UserRole) {
    state.app.dispatch_at(
        app::AppCommand::SetAuthSession(Some(AuthSession {
            source: identity::IdentitySource::SystemCurrentProcess,
            session_id: format!("{}-session", role.as_str()),
            user_id: format!("{}-id", role.as_str()),
            username: role.as_str().to_ascii_lowercase(),
            role,
            started_at_epoch_ms: 1,
        })),
        Instant::now(),
    );
}

fn system_status_test_snapshot(
    revision: u64,
    pressure: system_services::StoragePressure,
    link: bool,
    source: system_services::SystemVolumeSource,
) -> app::AppSystemStatusSnapshot {
    let sampled_at = Utc::now();
    app::AppSystemStatusSnapshot {
        revision,
        observed_at: sampled_at,
        metrics: system_services::SystemMetricsSnapshot::loading(),
        storage: system_services::StorageState::Ready(system_services::StorageSnapshot {
            volumes: vec![system_services::StorageVolumeSnapshot {
                identifier: "/sensitive/mount".into(),
                label: Some("secret-label".into()),
                kind: system_services::StorageVolumeKind::Fixed,
                is_system: true,
                access: system_services::StorageVolumeAccess::ReadWrite,
                total_bytes: Some(1000),
                available_bytes: Some(100),
                pressure,
            }],
            overall_pressure: pressure,
            system_volume_index: Some(0),
            system_volume_source: source,
            sampled_at,
        }),
        network: system_services::NetworkState::Ready(system_services::NetworkSnapshot {
            interfaces: vec![system_services::NetworkInterfaceSnapshot {
                name: "secret-iface".into(),
                display_name: Some("secret-display".into()),
                kind: system_services::NetworkInterfaceKind::Virtual,
                link_state: if link {
                    system_services::NetworkLinkState::Up
                } else {
                    system_services::NetworkLinkState::Down
                },
                addresses: vec!["192.0.2.99".into(), "2001:db8::99".into()],
            }],
            active_link_count: usize::from(link),
            has_active_link: link,
            sampled_at,
        }),
    }
}

fn system_status_metric_snapshot(uptime: u64) -> app::AppSystemStatusSnapshot {
    let mut snapshot = system_status_test_snapshot(
        uptime,
        system_services::StoragePressure::Normal,
        true,
        system_services::SystemVolumeSource::Detected,
    );
    snapshot.metrics.uptime =
        system_services::MetricState::Ready(system_services::UptimeSnapshot { seconds: uptime });
    snapshot.metrics.cpu = system_services::MetricState::Ready(system_services::CpuSnapshot {
        usage_percent: 25.0,
        per_core_percent: vec![25.0],
        logical_core_count: 1,
        physical_core_count: Some(1),
    });
    snapshot.metrics.memory =
        system_services::MetricState::Ready(system_services::MemorySnapshot {
            total_bytes: 100,
            used_bytes: 40,
            available_bytes: 60,
            swap_total_bytes: 0,
            swap_used_bytes: 0,
        });
    snapshot.metrics.network_io =
        system_services::MetricState::Ready(system_services::NetworkIoSnapshot {
            interfaces: vec![],
            total_received_bytes: 1,
            total_transmitted_bytes: 2,
            total_received_bytes_per_second: 3.0,
            total_transmitted_bytes_per_second: 4.0,
        });
    snapshot.metrics.thermal =
        system_services::MetricState::Ready(vec![system_services::ThermalSensorSnapshot {
            label: "CPU".into(),
            temperature_celsius: 50.0,
            critical_celsius: Some(90.0),
        }]);
    snapshot
}

fn explorer_routing_test_state() -> ShellSession {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    state.set_navigation_path(vec![ShellScreen::Explorer]);
    state.pages.common.focused_component = ShellComponent::Explorer;
    state.replace_explorer_state(Some(ExplorerState::new(".", false)));
    state.refresh_hit_map();
    state
}

fn explorer_terminal_test_fixture() -> (
    ShellSession,
    platform::mock::MockPlatform,
    SystemStatusTempGuard,
) {
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let temp = std::env::temp_dir().canonicalize().unwrap();
    let root = temp.join(format!(
        "tundra-shell-explorer-terminal-{}-{unique}",
        std::process::id()
    ));
    let guard = SystemStatusTempGuard(root.clone());
    let directory = root.join("中文 空格 & $ ` ' (folder)");
    std::fs::create_dir_all(&directory).unwrap();
    let mut explorer = ExplorerState::new(&directory, false);
    for name in ["first.txt", "second.txt"] {
        let path = directory.join(name);
        std::fs::write(&path, "fixture").unwrap();
        explorer.all_entries.push(app::explorer::ExplorerEntry {
            name: name.into(),
            path: path.clone(),
            trash_id: None,
            original_path: None,
            kind: app::explorer::ExplorerEntryKind::File,
            size: 7,
            modified: None,
            attributes: FileAttributes {
                path,
                is_file: true,
                is_dir: false,
                len: 7,
                readonly: false,
                modified: None,
                hidden: false,
                system: false,
                archive: false,
                symlink: false,
                junction: false,
                reparse_point: false,
                shortcut: false,
            },
            open_policy: platform::FileOpenPolicy::SystemDefault,
            type_label: "Text".into(),
            icon_key: "file".into(),
            metadata_warning: None,
        });
    }
    explorer.apply_projection();
    explorer.select_all();
    let mut state = explorer_routing_test_state();
    set_test_auth_role(&mut state, UserRole::Admin);
    state.set_navigation_path(vec![ShellScreen::Home, ShellScreen::Explorer]);
    state.replace_explorer_state(Some(explorer));
    state.refresh_hit_map();
    let dirs = platform::UserDirs::new(
        root.join("Desktop"),
        root.join("Documents"),
        root.join("Downloads"),
        root.join("Pictures"),
        root.join("Videos"),
        root.join("Music"),
        root.join("AppData"),
    )
    .unwrap();
    let paths = platform::build_windows_app_paths(
        root.join("Roaming"),
        root.join("Local"),
        root.join("Temp"),
    )
    .unwrap();
    (state, platform::mock::MockPlatform::new(dirs, paths), guard)
}

fn explorer_test_layout(state: &ShellSession) -> ui::ExplorerLayout {
    let terminal = Rect::new(0, 0, state.terminal_size.0, state.terminal_size.1);
    let ui::ShellLayout::Full { main, .. } = state.shell_layout_for(terminal) else {
        panic!("expected full shell layout");
    };
    ui::explorer_layout(main, &state.to_explorer_view_model())
}

fn hit_region_center(state: &ShellSession, component: ShellComponent) -> CellPosition {
    let area = state
        .hit_map
        .regions()
        .iter()
        .find(|region| region.component == component)
        .unwrap_or_else(|| panic!("missing {component:?} hit region"))
        .area;
    (
        area.x.saturating_add(area.width / 2),
        area.y.saturating_add(area.height / 2),
    )
}
