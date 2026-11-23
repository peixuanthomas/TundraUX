use crate::TundraTheme;
use crate::components::{Button, ButtonEmphasis};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

fn render(button: &Button, width: u16) -> Buffer {
    let area = Rect::new(0, 0, width, 3);
    let mut buffer = Buffer::empty(area);
    button.render(area, &mut buffer, &TundraTheme::default_dark());
    buffer
}

#[test]
fn primary_normal_uses_readable_text_on_accent_fill_and_border() {
    let theme = TundraTheme::default_dark();
    let button = Button::new("save", "Save").with_emphasis(ButtonEmphasis::Primary);
    let buffer = render(&button, 12);
    assert_eq!(
        buffer[(1, 1)].fg,
        theme.tokens().filled_style(theme.accent_color).fg.unwrap()
    );
    assert_eq!(buffer[(1, 1)].bg, theme.accent_color);
    assert_eq!(buffer[(0, 1)].fg, theme.accent_color);
    assert!(
        buffer[(1, 1)]
            .modifier
            .contains(ratatui::style::Modifier::BOLD)
    );
}

#[test]
fn danger_normal_uses_error_label_and_border() {
    let theme = TundraTheme::default_dark();
    let button = Button::new("delete", "Delete").with_emphasis(ButtonEmphasis::Danger);
    let buffer = render(&button, 12);
    assert_eq!(buffer[(1, 1)].fg, theme.error);
    assert_eq!(buffer[(0, 1)].fg, theme.error);
}

#[test]
fn danger_outline_stays_readable_and_stable_through_hover_and_press() {
    use crate::ColorCapability;
    use ratatui::style::Color;

    let area = Rect::new(0, 0, 6, 1);
    for capability in [ColorCapability::TrueColor, ColorCapability::Ansi] {
        let mut theme = TundraTheme::default_dark();
        theme.color_capability = capability;
        let mut button = Button::new("delete", r"\_/").with_emphasis(ButtonEmphasis::Danger);
        let mut colors = Vec::new();
        for (hovered, active) in [(false, false), (true, false), (true, true)] {
            button.state.hovered = hovered;
            button.state.active = active;
            let mut buffer = Buffer::empty(area);
            button.render_borderless(area, &mut buffer, &theme);
            let label = buffer
                .content()
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>();
            assert_eq!(label.trim(), r"\_/");
            let cell = &buffer[(1, 0)];
            assert_eq!(crate::theme::readable_foreground(cell.fg, cell.bg), cell.fg);
            colors.push((cell.fg, cell.bg));
        }
        assert!(colors.windows(2).all(|pair| pair[0] != pair[1]));
        if capability == ColorCapability::Ansi {
            assert_eq!(colors[0].0, Color::LightRed);
            assert_eq!(colors[1].1, Color::Red);
            assert_eq!(colors[2].1, Color::LightRed);
        }
    }
}

#[test]
fn focus_fill_changes_background_but_keeps_danger_text() {
    let theme = TundraTheme::default_dark();
    let mut button = Button::new("delete", "Delete")
        .with_emphasis(ButtonEmphasis::Danger)
        .with_focus_fill(Some(ratatui::style::Color::Blue));
    button.state.focused = true;
    let buffer = render(&button, 12);
    assert_eq!(buffer[(1, 1)].fg, theme.error);
    assert_eq!(buffer[(1, 1)].bg, ratatui::style::Color::Blue);
}

#[test]
fn disabled_wins_over_emphasis_and_focus() {
    let mut button = Button::new("delete", "Delete")
        .with_emphasis(ButtonEmphasis::Danger)
        .with_focus_fill(Some(ratatui::style::Color::Blue));
    button.state.focused = true;
    button.set_disabled(true);
    let buffer = render(&button, 12);
    assert_eq!(buffer[(1, 1)].fg, ratatui::style::Color::DarkGray);
    assert_ne!(buffer[(1, 1)].bg, ratatui::style::Color::Blue);
}
