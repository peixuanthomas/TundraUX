use super::*;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

#[track_caller]
fn wait_until(mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "package worker timed out");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn install(session: &mut ShellSession, names: &[&str], allow: bool) {
    let targets = names
        .iter()
        .map(|name| platform::management::selection::Target {
            id: format!("formula:{name}"),
            identity: BTreeMap::from([("name".into(), name.to_string())]),
        })
        .collect::<Vec<_>>();
    let command = ManagementCommand {
        kind: ManagementKind::Packages,
        action: "install".into(),
        target: None,
        identity: Default::default(),
        values: BTreeMap::from([(
            platform::management::selection::KEY.into(),
            serde_json::to_string(&targets).unwrap(),
        )]),
    };
    session.start_management_operation(Some(command), false, true, None, Some("Homebrew".into()));
    session
        .pages
        .management
        .management_state
        .auto_admin_job
        .as_ref()
        .unwrap()
        .decide(allow);
    wait_until(|| {
        session.poll_management();
        session
            .pages
            .management
            .management_state
            .operation_job
            .is_none()
    });
}

fn package_session(root: &Path, watchdog: &AppWatchdog, scope: &str) -> ShellSession {
    let mut session = package_list_state();
    session.storage_manager = Some(
        StorageManager::open(platform::build_binary_dir_app_paths(root).unwrap())
            .unwrap()
            .manager,
    );
    session.settings_task_runtime = ShellSettingsTaskRuntime::new_managed(watchdog.clone());
    let state = &mut session.pages.management.management_state;
    state.query.as_mut().unwrap().scope = scope.into();
    state.snapshot =
        platform::management::query(state.query.as_ref().unwrap(), &AtomicBool::new(false))
            .unwrap();
    state.selected = 0;
    while session.notification_dismiss_active_modal_without_response() {}
    session
}

fn activate(session: &mut ShellSession, id: &str, input: &str) {
    let index = session
        .management_actions()
        .iter()
        .position(|(action, _)| action.id == id)
        .unwrap();
    session.refresh_hit_map();
    let layout = ui::management_layout(
        session.management_main(),
        &session.to_management_view_model(),
    );
    let rect = layout.actions[index];
    assert!(
        !rect.is_empty() && rect.right() <= 108 && rect.bottom() <= 20,
        "{rect:?}"
    );
    if input == "keyboard" {
        for _ in 0..6 {
            if session.pages.management.management_state.workspace_focus
                == ui::components::WorkspaceFocus::Actions
            {
                break;
            }
            session.apply_input(InputEvent::key(InputKey::Tab));
        }
        assert_eq!(
            session.pages.management.management_state.selected_action,
            index
        );
        session.apply_input(InputEvent::key(InputKey::Enter));
    } else {
        let point = (rect.x, rect.y);
        if input == "mouse" {
            session.apply_input(InputEvent::mouse_moved(point));
        }
        session.apply_input(InputEvent::mouse_down(PointerButton::Left, point));
        assert!(session.pages.management.management_state.form.is_none());
        session.apply_input(InputEvent::mouse_up(PointerButton::Left, point));
    }
}

fn activate_result(session: &mut ShellSession, input: &str) {
    wait_until(|| {
        session.poll_auto_admin();
        session
            .auto_admin_view()
            .is_some_and(|model| model.add_to_launcher)
    });
    let model = session.auto_admin_view().unwrap();
    let button = ui::auto_admin_layout_in(session.auto_admin_available_area(), &model).buttons[1];
    assert!(!button.is_empty() && button.right() <= 108 && button.bottom() <= 20);
    if input == "keyboard" {
        session.apply_input(InputEvent::key(InputKey::Tab));
        session.apply_input(InputEvent::key(InputKey::Enter));
    } else {
        let point = (button.x, button.y);
        if input == "mouse" {
            session.apply_input(InputEvent::mouse_moved(point));
        }
        session.apply_input(InputEvent::mouse_down(PointerButton::Left, point));
        assert!(session.auto_admin_visible());
        session.apply_input(InputEvent::mouse_up(PointerButton::Left, point));
    }
    assert!(!session.auto_admin_visible());
}

#[test]
fn homebrew_package_launcher_from_install_to_saved_entry_at_minimum_size() {
    const FIXTURE: &str = "TUNDRA_TEST_PACKAGE_LAUNCHER_ROOT";
    let Some(root) = std::env::var_os(FIXTURE).map(PathBuf::from) else {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let brew = root.join("brew");
        fs::write(&brew, r#"#!/bin/sh
cd -- "$(dirname -- "$0")" || exit 1
case "$1" in
 info)
    printf '{"formulae":['
    sep=''
    for name in demo suite library broken; do
      if test -f "$name.installed"; then installed='[{"version":"1"}]'
      elif test "$3" = "--installed"; then continue
      else installed='[]'; fi
      printf '%s{"full_name":"%s","installed":%s,"versions":{"stable":"1"}}' "$sep" "$name" "$installed"
      sep=,
    done
    printf '],"casks":[]}\n' ;;
 search) exit 0 ;;
 install) shift 3; for name do test "$name" != broken || exit 3; touch "$name.installed"; done ;;
 --prefix) test "$2" = "--installed" && test -f "$3.installed" && printf '%s/%s\n' "$PWD" "$3" ;;
 *) exit 2 ;;
esac
"#).unwrap();
        fs::set_permissions(&brew, fs::Permissions::from_mode(0o755)).unwrap();
        for relative in [
            "demo/bin/only command",
            "suite/bin/first",
            "suite/bin/second",
        ] {
            let path = root.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, "#!/bin/sh\n").unwrap();
            fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
        }
        // Run the native discovery and installer against this PATH without changing other Rust tests.
        let paths = std::env::join_paths(
            std::iter::once(root.to_owned())
                .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
        )
        .unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                std::thread::current().name().unwrap(),
                "--nocapture",
            ])
            .env(FIXTURE, root)
            .env("PATH", paths)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(root.join("demo.installed").exists());
        return;
    };

    let (runtime, process) =
        watchdog::WatchdogRuntime::start_isolated(watchdog::WatchdogConfig::new(
            root.join("reports"),
            root.join("fallback"),
            root.join("state"),
            "package-launcher-test",
            "test",
        ))
        .unwrap();
    let watchdog = process.register_app(shell_watchdog_descriptor()).unwrap();
    let platform = platform::native_platform();
    let mut session = package_session(&root.join("results"), &watchdog, "search");
    install(&mut session, &["demo"], false);
    assert!(!root.join("demo.installed").exists());
    assert!(session.package_launcher_actions().is_empty());
    install(&mut session, &["broken"], true);
    assert!(session.package_launcher_actions().is_empty());
    install(&mut session, &["demo", "suite", "library"], true);
    assert_eq!(
        session
            .pages
            .management
            .management_state
            .just_installed
            .len(),
        3
    );
    while session.notification_dismiss_active_modal_without_response() {}
    session.apply_input(InputEvent::Resize {
        width: 108,
        height: 20,
    });
    activate_result(&mut session, "keyboard");
    assert_eq!(
        session
            .to_management_view_model()
            .form
            .unwrap()
            .choice
            .unwrap()
            .values
            .len(),
        3
    );
    session.handle_management_paste("suite");
    session.handle_management_key(&KeyInput::new(InputKey::Enter));
    assert!(
        matches!(session.pages.management.management_state.form.as_ref().map(|form| &form.purpose), Some(FormPurpose::PackageLookup(name)) if name == "suite")
    );
    wait_until(|| session.poll_package_launcher());
    let choices = session
        .to_management_view_model()
        .form
        .unwrap()
        .choice
        .unwrap();
    assert_eq!(choices.values.len(), 2);
    session.handle_management_paste("second");
    session.handle_management_key(&KeyInput::new(InputKey::Enter));
    let form = session
        .pages
        .launcher
        .launcher_run_settings
        .as_ref()
        .unwrap();
    assert_eq!(form.name.value(), "suite");
    assert!(form.arguments.value().ends_with("/suite/bin/second"));
    session.handle_launcher_run_settings_key(&KeyInput::new(InputKey::Escape), platform.as_ref());
    assert!(
        session
            .storage_manager
            .as_ref()
            .unwrap()
            .load_config()
            .unwrap()
            .launcher
            .entries
            .iter()
            .all(|entry| entry.custom.is_none())
    );

    let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("../ascii-assets/assets");
    for language in ["en-US", "zh-CN"] {
        for input in ["keyboard", "mouse", "touch"] {
            let mut session = package_session(
                &root.join(format!("{language}-{input}")),
                &watchdog,
                "installed",
            );
            session.language = Arc::new(
                i18n::LanguageSnapshot::load(&assets, language, 1)
                    .unwrap()
                    .snapshot,
            );
            let _language = i18n::enter_snapshot(session.language.clone());
            session.apply_input(InputEvent::Resize {
                width: 108,
                height: 20,
            });
            let name = if input == "touch" { "library" } else { "demo" };
            fs::remove_file(root.join(format!("{name}.installed"))).unwrap();
            install(&mut session, &[name], true);
            activate_result(&mut session, input);
            session.handle_management_key(&KeyInput::new(InputKey::Escape));
            assert!(session.pages.management.management_state.form.is_none());
            assert!(!session.poll_package_launcher());
            let state = &mut session.pages.management.management_state;
            state.selected = state
                .snapshot
                .rows
                .iter()
                .position(|row| row.id == format!("formula:{name}"))
                .unwrap();
            assert_eq!(
                session.package_launcher_actions()[0].0.label,
                i18n::tr!("shell-add-to-launcher")
            );
            activate(&mut session, "package_launcher", input);
            assert!(
                matches!(
                    session
                        .pages
                        .management
                        .management_state
                        .form
                        .as_ref()
                        .map(|form| &form.purpose),
                    Some(FormPurpose::PackageLookup(_))
                ),
                "{input} in {language}"
            );
            wait_until(|| session.poll_package_launcher());
            let form = session
                .pages
                .launcher
                .launcher_run_settings
                .as_mut()
                .unwrap();
            assert_eq!(form.name.value(), name);
            if name == "library" {
                assert!(form.arguments.value().is_empty());
            } else {
                assert_eq!(
                    shell_words::split(form.arguments.value()).unwrap(),
                    vec![root.join("demo/bin/only command").to_str().unwrap()]
                );
            }
            form.name.set_value("我的工具");
            form.arguments.set_value("printf hello");
            session.save_launcher_run_settings(platform.as_ref());
            assert!(session.pages.launcher.launcher_run_settings.is_none());
            session.close_launcher();
            session.open_launcher(platform.as_ref());
            assert!(
                session
                    .app
                    .launcher_state()
                    .unwrap()
                    .items
                    .iter()
                    .any(
                        |item| item.record.custom.as_ref().is_some_and(|saved| saved.name
                            == "我的工具"
                            && saved.command == "printf hello")
                    )
            );
        }
    }
    drop(session);
    runtime.shutdown().unwrap();
}
