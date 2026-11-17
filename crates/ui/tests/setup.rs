#[path = "support/composition.rs"]
mod composition;
use composition as ui;
mod support;

use support::terminal_output;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ui::{
    SetupField, SetupStep, SetupViewModel, TundraTheme, setup_language_options,
    setup_timezone_options,
};

#[test]
fn short_setup_projects_only_visible_buttons_to_the_scrolled_rows() {
    let area = Rect::new(3, 2, 40, 10);
    let mut model = sample_model(SetupStep::Appearance, None);
    let theme = TundraTheme::default_dark();
    let buttons = ui::components::ButtonFrame::new(None, None, &theme);
    let mut context = ui::RenderContext::from_theme(&theme, Default::default(), Default::default());
    context.buttons = Some(buttons.clone());
    let mut terminal = Terminal::new(TestBackend::new(50, 15)).unwrap();
    terminal
        .draw(|frame| ui::render_setup_content(frame, area, &model, &context))
        .unwrap();
    assert!(
        !buttons
            .regions()
            .iter()
            .any(|button| button.id.as_str() == "setup.appearance.finish")
    );
    model.scroll_offset = u16::MAX;
    terminal
        .draw(|frame| ui::render_setup_content(frame, area, &model, &context))
        .unwrap();
    let viewport = ui::setup_viewport(area, &model);
    let expected = viewport
        .project(ui::setup_appearance_field_area(
            viewport.content,
            SetupField::AppearanceSubmit,
        ))
        .unwrap();
    assert!(
        buttons.regions().iter().any(
            |button| button.id.as_str() == "setup.appearance.finish" && button.area == expected
        )
    );
    assert!(
        buttons
            .regions()
            .iter()
            .all(|button| button.id.as_str() != "setup.exit")
    );
    assert!(viewport.scrollbar.is_some());
    assert_eq!(
        viewport.content_point((expected.x, expected.y)),
        Some((expected.x - area.x, expected.y - area.y + viewport.offset))
    );
}

#[test]
fn narrow_setup_navigation_keeps_previous_and_primary_shortcuts_visible() {
    let area = Rect::new(3, 2, 30, 10);
    let mut model = sample_model(SetupStep::Timezone, None);
    model.scroll_offset = u16::MAX;
    let viewport = ui::setup_viewport(area, &model);
    let [back, next] = ui::setup_navigation_areas(viewport.content);
    assert!(back.bottom() <= next.y);
    assert!(viewport.project(back).is_some() && viewport.project(next).is_some());
    let mut terminal = Terminal::new(TestBackend::new(40, 15)).unwrap();
    let context = ui::RenderContext::from_theme(
        &TundraTheme::default_dark(),
        Default::default(),
        Default::default(),
    );
    terminal
        .draw(|frame| ui::render_setup_content(frame, area, &model, &context))
        .unwrap();
    let output = terminal_output(&terminal);
    assert!(output.contains("Alt+← Back"));
    assert!(output.contains("F10 Next"));
}

fn sample_model(step: SetupStep, error: Option<String>) -> SetupViewModel {
    sample_model_with_timezone(step, "Asia/Tokyo", error)
}

fn sample_model_with_timezone(
    step: SetupStep,
    timezone_id: &str,
    error: Option<String>,
) -> SetupViewModel {
    let languages = setup_language_options();
    let timezones = setup_timezone_options();
    let selected_timezone_index = timezones
        .iter()
        .position(|timezone| timezone.id == timezone_id)
        .unwrap_or_else(|| panic!("{timezone_id} in setup catalog"));

    SetupViewModel {
        scroll_offset: 0,
        step,
        languages,
        timezones,
        selected_language_index: 0,
        selected_timezone_index,
        timezone_window_start: selected_timezone_index.saturating_sub(2),
        focused_field: SetupField::AppearanceShape,
        defaults_available: false,
        border_shape: ui::BorderShape::Rounded,
        theme_color: Color::White,
        theme_color_value: "white".to_string(),
        accent_color: Color::Cyan,
        accent_color_value: "cyan".to_string(),
        custom_color_target: None,
        custom_color_editing: Default::default(),
        custom_color_input: String::new(),
        custom_color_valid: false,
        custom_color_conflicts_with_theme: false,
        custom_color_error: None,
        error,
    }
}

#[test]
fn defaults_action_stays_visible_below_scrolling_setup_at_minimum_size() {
    let mut model = sample_model(SetupStep::Appearance, None);
    model.defaults_available = true;
    let area = ratatui::layout::Rect::new(0, 0, 108, 14);
    let button = ui::setup_defaults_area(area);
    for scroll in [0, u16::MAX] {
        model.scroll_offset = scroll;
        let viewport = ui::setup_viewport(area, &model);
        assert!(button.y >= viewport.area.bottom());
        assert!(button.bottom() <= area.bottom());
    }
}

#[test]
fn appearance_values_remain_current_while_only_the_active_field_has_keyboard_focus() {
    let mut model = sample_model(SetupStep::Appearance, None);
    model.focused_field = SetupField::AppearanceThemeColor;
    let main = Rect::new(0, 1, 108, 18);
    let capture = ui::automation::FrameCapture::begin(Rect::new(0, 0, 108, 20), "setup");
    let mut terminal = Terminal::new(TestBackend::new(108, 20)).unwrap();
    terminal
        .draw(|frame| ui::render_setup_content(frame, main, &model, &ui::RenderContext::default()))
        .unwrap();
    let snapshot = capture.finish(terminal.backend().buffer());
    let shape = snapshot
        .nodes
        .iter()
        .find(|node| node.id.ends_with("/setup.appearance.shape.rounded"))
        .unwrap();
    assert!(shape.selected && !shape.focused);
    let selected_color = snapshot
        .nodes
        .iter()
        .find(|node| node.id.ends_with("/setup.appearance.theme.white"))
        .unwrap();
    assert!(selected_color.selected && selected_color.focused);
    assert_eq!(
        snapshot
            .nodes
            .iter()
            .filter(|node| node.role == "button" && node.focused)
            .count(),
        1
    );
}
