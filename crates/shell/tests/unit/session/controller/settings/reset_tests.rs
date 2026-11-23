use super::*;
use ratatui::{Terminal, backend::TestBackend};

struct ResetTestRoot(PathBuf);

impl Drop for ResetTestRoot {
    fn drop(&mut self) {
        let _ = platform::cleanup_temp_path(&self.0);
    }
}

#[test]
fn settings_reset_uses_rendered_button_and_records_only_explicit_confirmation() {
    let mut state = ShellSession::new_for_home_mode(
        ShellLaunchConfig::default(),
        (108, 20),
        ShellHomeMode::User,
    );
    state.request_settings_reset_all_confirmation();
    assert!(state.to_notification_view_model().is_none());
    assert!(!state.reset_requested);

    state.app.dispatch_at(
        app::AppCommand::SetAuthSession(Some(AuthSession {
            source: identity::IdentitySource::SystemCurrentProcess,
            session_id: "fixture-session".into(),
            user_id: "fixture".into(),
            username: "fixture".into(),
            role: UserRole::User,
            started_at_epoch_ms: 1,
        })),
        Instant::now(),
    );
    let unique = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
        "tundra-settings-reset-{}-{unique}",
        std::process::id()
    ));
    std::fs::create_dir(&root).unwrap();
    let root = ResetTestRoot(root);
    let paths = platform::build_windows_app_paths(
        root.0.join("Roaming"),
        root.0.join("Local"),
        root.0.join("Temp"),
    )
    .unwrap();
    state.storage_manager = Some(StorageManager::open(paths).unwrap().manager);
    state.open_settings();
    state.select_settings_category(ui::SettingsCategory::Reset);
    state.apply_input(InputEvent::key(InputKey::Enter));
    assert!(state.to_notification_view_model().is_some());
    state.apply_input(InputEvent::key(InputKey::Enter));
    assert!(!state.reset_requested);

    let model = state.to_settings_view_model().unwrap();
    let layout = ui::settings_layout(state.touch_main(), &model);
    let mut context = ui::RenderContext::default();
    let buttons = ui::components::ButtonFrame::new(None, None, &context.compatibility_theme());
    context.buttons = Some(buttons.clone());
    let mut terminal = Terminal::new(TestBackend::new(108, 20)).unwrap();
    terminal
        .draw(|frame| ui::render_settings_content(frame, &layout, &model, &context))
        .unwrap();
    state.button_regions = buttons.regions();
    let button = state
        .button_regions
        .iter()
        .find(|region| region.id.as_str() == "settings.field.resetall")
        .unwrap()
        .area;
    let point = (button.x + button.width / 2, button.y);
    let outside = (button.x - 1, button.y);
    let now = Instant::now();
    state.apply_input_at(InputEvent::mouse_down(PointerButton::Left, point), now);
    assert!(state.to_notification_view_model().is_none());
    state.apply_input_at(InputEvent::mouse_drag(PointerButton::Left, outside), now);
    state.apply_input_at(InputEvent::mouse_up(PointerButton::Left, outside), now);
    assert!(state.to_notification_view_model().is_none());

    state.apply_input_at(InputEvent::mouse_down(PointerButton::Left, point), now);
    assert!(state.to_notification_view_model().is_none());
    state.apply_input_at(InputEvent::mouse_up(PointerButton::Left, point), now);
    let modal = state
        .to_notification_view_model()
        .expect("reset confirmation after release");
    assert!(
        modal
            .actions
            .iter()
            .any(|action| action.cancel && action.selected)
    );
    assert!(!state.reset_requested);

    state.apply_input(InputEvent::key(InputKey::Tab));
    state.apply_input(InputEvent::key(InputKey::Enter));
    assert!(state.reset_requested);
    assert!(state.to_notification_view_model().is_none());
}
