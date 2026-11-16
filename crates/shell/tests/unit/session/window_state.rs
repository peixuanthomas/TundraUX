use super::*;

fn area() -> WorkspaceLayout {
    WorkspaceLayout::new(
        ratatui::layout::Rect::new(0, 0, 217, 39),
        ratatui::layout::Rect::new(0, 1, 217, 37),
        &[],
        &[],
        None,
        None,
    )
}

#[test]
fn opening_replaces_focused_slot_and_background_stays_explicit() {
    let (a, b, c) = (SessionId(1), SessionId(2), SessionId(3));
    let mut windows = WindowSelection::default();
    windows.open(a);
    windows.split(a, WorkspaceSplitMode::LeftRight);
    windows.open(b);
    windows.open(c);
    assert_eq!(windows.visible(), vec![a, c]);
    assert!(!windows.includes(b));
    windows.focus(a);
    windows.switch(b);
    assert_eq!(windows.visible(), vec![b, c]);
}

#[test]
fn minimizing_merges_the_empty_region_and_restoring_uses_its_previous_slot() {
    let (a, b) = (SessionId(1), SessionId(2));
    let mut windows = WindowSelection::default();
    windows.open(a);
    windows.split(a, WorkspaceSplitMode::LeftRight);
    windows.open(b);
    windows.minimize(a);
    assert_eq!(windows.visible(), vec![b]);
    windows.activate(a, &area());
    assert_eq!(windows.visible(), vec![a, b]);
}

#[test]
fn cancel_split_preserves_other_windows_and_fullscreen_discards_the_split() {
    let (a, b) = (SessionId(1), SessionId(2));
    let mut windows = WindowSelection::default();
    windows.open(a);
    windows.split(a, WorkspaceSplitMode::LeftRight);
    windows.open(b);
    let previous = windows.tree.clone();
    windows.split(b, WorkspaceSplitMode::TopBottom);
    assert!(windows.cancel_add());
    assert_eq!(windows.tree, previous);
    assert_eq!(windows.focused(), Some(b));
    windows.fullscreen(a);
    assert_eq!(windows.visible(), vec![a]);
    windows.activate(b, &area());
    assert_eq!(windows.visible(), vec![b]);
}

#[test]
fn window_prefix_uses_ctrl_w_and_letter_directions_and_never_repeats_a_split() {
    let mut prefix = WindowPrefix::default();
    let key = |label: &str| {
        if let Some(letter) = label.strip_prefix("Ctrl+") {
            KeyInput::with_modifiers(
                InputKey::Char(letter.chars().next().unwrap()),
                crate::InputModifiers::CTRL,
            )
        } else if let crate::InputEvent::Key(key) = crate::InputEvent::from_key_label(label) {
            key
        } else {
            unreachable!()
        }
    };
    assert_eq!(prefix.route(&key("Ctrl+b")), WindowShortcut::Pass);
    assert_eq!(prefix.route(&key("Ctrl+w")), WindowShortcut::Consume);
    assert_eq!(
        prefix.route(&key("v")),
        WindowShortcut::Split(WorkspaceSplitMode::LeftRight)
    );
    assert_eq!(prefix.route(&key("s")), WindowShortcut::Pass);
    for (label, command) in [
        ("s", WindowShortcut::Split(WorkspaceSplitMode::TopBottom)),
        ("h", WindowShortcut::MoveFocus(-1, 0)),
        ("l", WindowShortcut::MoveFocus(1, 0)),
        ("j", WindowShortcut::MoveFocus(0, 1)),
        ("k", WindowShortcut::MoveFocus(0, -1)),
        ("o", WindowShortcut::Maximize),
    ] {
        prefix.route(&key("Ctrl+w"));
        assert_eq!(prefix.route(&key(label)), command);
    }
    prefix.route(&key("Ctrl+w"));
    assert_eq!(
        prefix.route(&key("Ctrl+w")),
        WindowShortcut::SendPrefix(key("Ctrl+w"))
    );
    prefix.route(&key("Ctrl+w"));
    assert_eq!(prefix.route(&key("x")), WindowShortcut::Pass);
    prefix.route(&key("Ctrl+w"));
    assert_eq!(prefix.route(&key("X")), WindowShortcut::Kill);
    assert_eq!(
        prefix.route(&key("Ctrl+w").repeated()),
        WindowShortcut::Consume
    );
    prefix.route(&key("Ctrl+w"));
    assert_eq!(prefix.route(&key("Esc")), WindowShortcut::Consume);
    assert_eq!(prefix.route(&key("v")), WindowShortcut::Pass);
}
