use super::*;

#[test]
fn release_view_only_shows_metadata_until_an_update_is_available() {
    use ui::{SettingsField, SettingsUpdatePanel};
    let identity = app::update::BuildIdentity {
        package_version: "1.2.3".into(),
        commit_sha: Some("a".repeat(40)),
        dirty: false,
    };
    let mut state = checked_update_state(app::update::UpdateRelation::Identical);
    let (cards, view) = update_settings_view(&identity, &state, PlatformKind::Linux, true, true);
    assert_eq!(
        cards[0]
            .items
            .iter()
            .map(|item| item.field)
            .collect::<Vec<_>>(),
        [SettingsField::UpdateMode, SettingsField::InstalledVersion],
    );
    assert_eq!(cards[0].items[1].value, "1.2.3 (aaaaaaa)");
    assert_eq!(view.panel, SettingsUpdatePanel::Idle);
    assert!(view.action.is_none());

    state.check_result.as_mut().unwrap().relation =
        app::update::UpdateRelation::Behind { remote_ahead: 1 };
    let (cards, view) = update_settings_view(&identity, &state, PlatformKind::Linux, true, true);
    assert_eq!(cards[0].items[1].value, "1.2.3 (aaaaaaa) → 1.3.0 (bbbbbbb)");
    let SettingsUpdatePanel::Release { notes, activity } = view.panel else {
        panic!("expected a release offer");
    };
    assert_eq!(notes, "Release notes\nwith body");
    assert!(!activity.show_build_details);
    assert!(view.action.unwrap().enabled);
}

#[test]
fn prepared_update_waits_for_auto_admin_completion_or_rejection() {
    for approve in [false, true] {
        let mut state = ShellSession::new_for_home_mode(
            ShellLaunchConfig::default(),
            (120, 40),
            ShellHomeMode::User,
        );
        let (responses, _inputs) = mpsc::channel();
        let job = state
            .begin_auto_admin("System operation".into(), true, responses)
            .unwrap();
        let manifest = PathBuf::from("prepared-update.json");
        state
            .settings_task_runtime
            .shared
            .update_event_tx
            .send(SettingsUpdateTaskEvent::PrepareCompleted(Ok(
                manifest.clone()
            )))
            .unwrap();

        // Draining the prepared result must keep it while AA is still waiting.
        for _ in 0..2 {
            state.poll_settings_background_tasks();
            assert!(!state.shutdown_requested());
            assert_eq!(state.update_apply_manifest.as_ref(), Some(&manifest));
            assert_eq!(
                state.settings_update_state.phase,
                Some(app::update::UpdatePhase::WaitingForRestart)
            );
            assert_eq!(
                state.settings_update_state.status,
                i18n::LocalizedText::from(i18n::msg!("progress-phase-restart"))
            );
        }
        assert!(state.settings_task_runtime.drain_update_events().is_empty());
        assert!(
            state
                .settings_task_runtime
                .shared
                .update_worker
                .lock()
                .unwrap()
                .is_none()
        );

        if approve {
            assert!(state.handle_auto_admin_input(&InputEvent::key(InputKey::Tab)));
        }
        let key = if approve {
            InputKey::Enter
        } else {
            InputKey::Escape
        };
        assert!(state.handle_auto_admin_input(&InputEvent::key(key)));
        if approve {
            assert!(state.auto_admin_running());
            assert!(!state.auto_admin_view().unwrap().confirming);
            state.poll_settings_background_tasks();
            assert!(!state.shutdown_requested());
            assert_eq!(state.update_apply_manifest.as_ref(), Some(&manifest));
            job.finish(Ok("Done".into()));
        }

        assert!(!state.auto_admin_running());
        state.poll_settings_background_tasks();
        assert!(state.shutdown_requested());
        assert_eq!(state.update_apply_manifest.as_ref(), Some(&manifest));
        assert_eq!(
            state.settings_update_state.status,
            i18n::LocalizedText::from(i18n::msg!("settings-update-restarting"))
        );
    }
}

#[test]
fn prepared_update_without_auto_admin_still_restarts_immediately() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    let manifest = PathBuf::from("prepared-update.json");
    state
        .settings_task_runtime
        .shared
        .update_event_tx
        .send(SettingsUpdateTaskEvent::PrepareCompleted(Ok(
            manifest.clone()
        )))
        .unwrap();
    state.poll_settings_background_tasks();
    assert!(state.shutdown_requested());
    assert_eq!(state.update_apply_manifest.as_ref(), Some(&manifest));
}

#[test]
fn busy_update_disables_mode_switch_and_start() {
    let mut state = checked_update_state(app::update::UpdateRelation::Behind { remote_ahead: 1 });
    state.busy = true;
    let (cards, view) = update_settings_view(
        &app::update::current_build_identity(),
        &state,
        PlatformKind::Linux,
        true,
        true,
    );
    assert!(!cards[0].items[0].enabled);
    assert!(!view.action.unwrap().enabled);
}

fn settings_language_snapshots() -> Vec<std::sync::Arc<i18n::LanguageSnapshot>> {
    let root = std::env::temp_dir().join(format!(
        "tux-settings-locales-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let canonical =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ascii-assets/assets/locales");
    let mut snapshots = Vec::new();
    for code in ["en-US", "zh-CN"] {
        let locale = root.join("locales").join(code);
        std::fs::create_dir_all(locale.join("modules")).unwrap();
        for relative in [
            "manifest.toml",
            "modules/settings.ftl",
            "modules/ui-settings.ftl",
        ] {
            std::fs::copy(canonical.join(code).join(relative), locale.join(relative)).unwrap();
        }
        snapshots.push(std::sync::Arc::new(
            i18n::LanguageSnapshot::load(&root, code, 1)
                .unwrap()
                .snapshot,
        ));
    }
    std::fs::remove_dir_all(root).unwrap();
    snapshots
}

#[test]
fn saved_settings_and_validation_messages_rerender_with_their_arguments() {
    let saved: i18n::LocalizedText = settings_saved_field(ui::SettingsField::BorderShape).into();
    let category: i18n::LocalizedText = i18n::msg!(
        "settings-category-status",
        category = format!("{:?}", ui::SettingsCategory::RegionTime)
    )
    .into();
    let invalid = parse_editor_explorer_open_extensions("bad/suffix").unwrap_err();
    let relation = update_relation_label(&app::update::UpdateRelation::Diverged {
        remote_ahead: 2,
        local_ahead: 3,
    });
    let error: i18n::LocalizedText = i18n::msg!(
        "settings-update-failed",
        reason = i18n::msg!("settings-admin-required")
    )
    .into();
    let retained = (
        saved.clone(),
        category.clone(),
        invalid.clone(),
        relation.clone(),
    );
    for (snapshot, expected_saved, expected_category, expected_invalid, expected_relation) in
        settings_language_snapshots()
            .into_iter()
            .zip([
                (
                    "Saved Border shape",
                    "Settings: Region & Time",
                    "Invalid suffix",
                    "Builds diverged",
                ),
                (
                    "已保存边框形状",
                    "设置：区域与时间",
                    "无效后缀",
                    "构建已分叉",
                ),
            ])
            .map(|(snapshot, (saved, category, invalid, relation))| {
                (snapshot, saved, category, invalid, relation)
            })
    {
        let expected_error = if snapshot.code() == "en-US" {
            "Update failed: Administrator permission is required"
        } else {
            "更新失败：需要管理员权限"
        };
        let _language = i18n::enter_snapshot(snapshot);
        assert_eq!(saved.render_current(), expected_saved);
        assert_eq!(category.render_current(), expected_category);
        assert!(invalid.render_current().starts_with(expected_invalid));
        assert!(invalid.render_current().contains("bad/suffix"));
        let rendered_relation = relation.render_current();
        assert!(rendered_relation.starts_with(expected_relation));
        assert!(rendered_relation.contains('2'));
        assert!(rendered_relation.contains('3'));
        assert_eq!(error.render_current(), expected_error);
        assert_eq!(
            (&saved, &category, &invalid, &relation),
            (&retained.0, &retained.1, &retained.2, &retained.3)
        );
    }
}

#[test]
fn update_cards_and_picker_labels_rerender_without_changing_action_or_color_values() {
    let identity = app::update::BuildIdentity {
        package_version: "1.2.3".to_string(),
        commit_sha: Some("1111111111111111".to_string()),
        dirty: false,
    };
    let update = checked_update_state(app::update::UpdateRelation::Behind { remote_ahead: 1 });
    let picker = SettingsPickerState {
        editing: Default::default(),
        options: Vec::new(),
        kind: ui::SettingsPickerKind::BorderColor,
        query: String::new(),
        selected_index: 0,
        window_start: 0,
        image_icons_supported: false,
    };
    for (snapshot, (start_label, custom_label)) in settings_language_snapshots().into_iter().zip([
        ("Start update", "Custom color…"),
        ("开始更新", "自定义颜色…"),
    ]) {
        let _language = i18n::enter_snapshot(snapshot);
        let (_, view) = update_settings_view(&identity, &update, PlatformKind::Linux, true, true);
        let start = view.action.unwrap();
        assert_eq!(start.label, start_label);
        assert!(start.enabled);
        let options = settings_picker_options(&picker, &[]);
        let custom = options.last().unwrap();
        assert_eq!(custom.label, custom_label);
        assert_eq!(custom.detail, "#RRGGBB");
        assert_eq!(options[0].detail, "white");
    }
}

fn checked_update_state(relation: app::update::UpdateRelation) -> SettingsUpdateState {
    SettingsUpdateState {
        check_result: Some(app::update::UpdateCheckResult {
            release: Some(app::update::ReleaseUpdate {
                version: "1.3.0".into(),
                archive_name: String::new(),
                archive_url: String::new(),
                checksum_url: String::new(),
            }),
            git_tree: None,
            default_branch: "v1.3.0".into(),
            head_sha: "b".repeat(40),
            relation,
            commits: vec![app::update::UpdateCommit {
                sha: "b".repeat(40),
                message: "Release notes\nwith body".into(),
            }],
        }),
        ..Default::default()
    }
}

#[test]
fn update_settings_enable_install_only_for_supported_admin_builds() {
    let identity = app::update::BuildIdentity {
        package_version: "1.2.3".into(),
        commit_sha: Some("a".repeat(40)),
        dirty: false,
    };
    let update = checked_update_state(app::update::UpdateRelation::Behind { remote_ahead: 1 });
    for (supported, admin) in [(true, true), (true, false), (false, true)] {
        let (_, view) =
            update_settings_view(&identity, &update, PlatformKind::Linux, supported, admin);
        assert_eq!(view.action.unwrap().enabled, supported && admin);
    }
}

#[test]
fn beta_keeps_history_when_current_and_confirms_a_different_build() {
    use app::update::{BuildIdentity, UpdateGitCommit, UpdateGitRow, UpdateRelation};
    let identity = BuildIdentity {
        package_version: "1.2.3".into(),
        commit_sha: Some("b".repeat(40)),
        dirty: false,
    };
    let mut update = checked_update_state(UpdateRelation::Identical);
    update.mode = storage::LinuxUpdateMode::Beta;
    let older = "a".repeat(40);
    let check = update.check_result.as_mut().unwrap();
    check.release = None;
    check.git_tree = Some(Ok([UpdateGitRow {
        graph: "* ".into(),
        commit: Some(UpdateGitCommit {
            sha: older.clone(),
            branches: "another-branch".into(),
            subject: "Choose this commit".into(),
        }),
    }]
    .into()));
    let (_, view) = update_settings_view(&identity, &update, PlatformKind::Linux, true, true);
    assert!(
        matches!(view.panel, ui::SettingsUpdatePanel::Source { ref rows, ref activity, .. }
        if rows.len() == 1 && activity.show_build_details)
    );
    assert!(!view.action.unwrap().enabled);

    update.git_selected = Some(0);
    update.confirmed_sha = update.selected_build().map(str::to_owned);
    let (cards, view) = update_settings_view(&identity, &update, PlatformKind::Linux, true, true);
    assert!(cards[0].items[2].value.contains("bbbbbbb"));
    assert!(view.action.unwrap().enabled);
    assert!(view.confirmation.unwrap().body.contains(&older));
}

#[test]
fn settings_scroll_stops_at_the_content_boundaries() {
    assert_eq!(settings_scroll_offset(2_001, 6, 2_004), 2_004);
    assert_eq!(settings_scroll_offset(2_007, -6, 2_100), 2_001);
    assert_eq!(settings_scroll_offset(0, -6, 2_100), 0);
    assert_eq!(settings_scroll_offset(0, 6, 0), 0);
}

#[test]
fn background_update_completion_invalidates_without_input_or_a_hit_map_refresh() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (120, 40),
        ShellHomeMode::User,
    );
    state.settings_update_state.busy = true;
    let generation = state.hit_map().generation();
    state
        .settings_task_runtime
        .shared
        .update_event_tx
        .send(SettingsUpdateTaskEvent::CheckCompleted(Err(
            "completed with a background error".into(),
        )))
        .unwrap();
    assert!(
        state
            .poll_background_at(&platform::mock::UnsupportedPlatform, Instant::now())
            .1
    );
    assert!(!state.settings_update_state.busy);
    assert!(state.settings_update_state.error.is_some());
    assert_eq!(state.hit_map().generation(), generation);
    assert!(
        !state
            .poll_background_at(&platform::mock::UnsupportedPlatform, Instant::now())
            .1
    );
}
