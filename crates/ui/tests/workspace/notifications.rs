use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use ui::{
    NotificationActionViewModel, NotificationDialogLayout, NotificationLayout, NotificationLevel,
    NotificationTone, NotificationViewModel, TundraTheme, notification_layout,
    render_notification_overlay,
};

fn long_notification() -> NotificationViewModel {
    NotificationViewModel::new(
        "startup-repair",
        NotificationLevel::Modal,
        NotificationTone::Warning,
        "语言资源已修复",
        (0..45)
            .map(|index| format!("修复文件{index:02}：语言资源/默认翻译.ftl"))
            .chain(std::iter::once("最后一行：所有修复结果已列出".into()))
            .collect::<Vec<_>>()
            .join("\n"),
        vec![
            NotificationActionViewModel::new("repair-restart", "自动修复并重启").selected(true),
            NotificationActionViewModel::new("exit", "退出"),
        ],
    )
}

fn layout(area: Rect, model: &NotificationViewModel) -> NotificationDialogLayout {
    match notification_layout(area, model) {
        NotificationLayout::Dialog(layout) => layout,
        other => panic!("expected scrollable notification, got {other:?}"),
    }
}
fn draw(area: Rect, model: &NotificationViewModel, theme: &TundraTheme) -> Terminal<TestBackend> {
    let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
    terminal
        .draw(|frame| render_notification_overlay(frame, area, model, theme))
        .unwrap();
    terminal
}
fn text(terminal: &Terminal<TestBackend>, area: Rect) -> String {
    let buffer = terminal.backend().buffer();
    (area.y..area.bottom())
        .map(|y| {
            (area.x..area.right())
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
                .replace(' ', "")
        })
        .collect::<Vec<_>>()
        .join("\n")
}
fn assert_actions_accessible(layout: &NotificationDialogLayout) {
    assert!(!layout.actions.is_empty());
    for (index, action) in layout.actions.iter().enumerate() {
        assert_eq!(action.index, index);
        assert!(action.area.width > 0 && action.area.height > 0);
        assert!(action.area.x >= layout.dialog.x + 1);
        assert!(action.area.right() < layout.dialog.right());
        assert!(action.area.intersection(layout.message).is_empty());
        assert!(action.area.bottom() <= layout.dialog.bottom());
        // The exact rectangles consumed by shell hit testing remain disjoint.
        for other in &layout.actions[index + 1..] {
            assert!(action.area.intersection(other.area).is_empty());
        }
    }
}

#[test]
fn long_messages_scroll_at_50_by_12_with_fixed_actions() {
    let area = Rect::new(0, 0, 50, 12);
    let error = NotificationViewModel::new(
        "backend-error",
        NotificationLevel::Modal,
        NotificationTone::Error,
        "Cannot read",
        concat!(
            "Reading system settings.\n",
            "The backend rejected this operation because permission to read the configuration is unavailable.\n",
            "The requested service remains active.\n",
            "No files have been changed.\n",
            "You can retry later.\n",
            "Close this window to return to Settings.\n",
            "Last line: files were untouched.",
        ),
        vec![NotificationActionViewModel::new("close", "Close").with_shortcut("Esc")],
    );
    let theme = TundraTheme::default_dark();
    let square = theme.clone().with_border_shape(ui::BorderShape::Square);
    for (mut model, start, end, complete_words) in [
        (
            long_notification(),
            "修复文件00",
            "最后一行：所有修复结果已列出",
            &[][..],
        ),
        (
            error,
            "Readingsystemsettings",
            "fileswereuntouched",
            &[
                "operation",
                "because",
                "permission",
                "configuration",
                "unavailable",
            ][..],
        ),
    ] {
        let first = layout(area, &model);
        assert_eq!(first.scroll_offset, 0);
        assert!(first.max_scroll_offset > 0);
        assert!(first.scrollbar.is_some());
        assert_actions_accessible(&first);
        let assert_action_labels =
            |terminal: &Terminal<TestBackend>, layout: &NotificationDialogLayout| {
                for action in &layout.actions {
                    assert!(
                        text(terminal, action.area).contains(&model.actions[action.index].label)
                    );
                }
            };
        for theme in [&theme, &square] {
            let terminal = draw(area, &model, theme);
            let output = text(&terminal, first.message);
            assert!(output.contains(start), "{output}");
            assert!(!output.contains(end), "{output}");
            assert_action_labels(&terminal, &first);
        }
        if !complete_words.is_empty() {
            let mut rendered = String::new();
            for offset in 0..=first.max_scroll_offset {
                model.scroll_offset = offset;
                let current = layout(area, &model);
                let terminal = draw(area, &model, &theme);
                rendered.push_str(&text(&terminal, current.message));
                rendered.push('\n');
            }
            for &word in complete_words {
                assert!(
                    rendered.contains(word),
                    "missing complete word {word}: {rendered}"
                );
            }
        }
        model.scroll_offset = usize::MAX;
        let last = layout(area, &model);
        assert_eq!(last.scroll_offset, last.max_scroll_offset);
        assert_eq!(last.actions, first.actions);
        assert_eq!(last.dialog, first.dialog);
        assert_actions_accessible(&last);
        let terminal = draw(area, &model, &theme);
        let output = text(&terminal, last.message);
        assert!(output.contains(end), "{output}");
        assert!(!output.contains(start), "{output}");
        assert_action_labels(&terminal, &last);
    }
}

#[test]
fn notification_dialog_keeps_its_semantic_tone_in_the_title() {
    let area = Rect::new(0, 0, 80, 24);
    let theme = TundraTheme::default_dark();
    for (tone, color) in [
        (NotificationTone::Success, theme.tokens().success),
        (NotificationTone::Warning, theme.tokens().warning),
        (NotificationTone::Error, theme.tokens().danger),
    ] {
        let mut model = long_notification();
        model.tone = tone;
        let layout = layout(area, &model);
        let terminal = draw(area, &model, &theme);
        let buffer = terminal.backend().buffer();
        let title = Rect::new(
            layout.message.x,
            layout.dialog.y,
            layout.message.width,
            layout.message.y - layout.dialog.y,
        );
        assert!(
            title
                .positions()
                .any(|position| buffer[position].symbol() == "语" && buffer[position].fg == color)
        );
        assert_eq!(
            buffer[(layout.dialog.x, layout.dialog.y)].bg,
            theme.tokens().surface
        );
        assert_eq!(buffer[(layout.dialog.x, layout.dialog.y)].symbol(), " ");
    }
}

#[test]
fn notification_scroll_clamps_on_resize_and_when_the_message_becomes_short() {
    let mut model = long_notification();
    model.scroll_offset = usize::MAX;
    let small = layout(Rect::new(0, 0, 50, 12), &model);
    let large = layout(Rect::new(0, 0, 80, 30), &model);
    assert!(large.message.height > small.message.height);
    assert!(large.max_scroll_offset < small.max_scroll_offset);
    assert_eq!(large.scroll_offset, large.max_scroll_offset);
    assert_actions_accessible(&large);
    model.message = "修复已完成".into();
    let short = layout(Rect::new(0, 0, 50, 12), &model);
    assert_eq!(short.scroll_offset, 0);
    assert_eq!(short.max_scroll_offset, 0);
    assert!(short.scrollbar.is_none());
    assert!(short.scroll_hint.is_none());
}

#[test]
fn wrapped_and_stacked_actions_keep_their_own_space_while_the_message_scrolls() {
    let area = Rect::new(0, 0, 50, 12);
    let mut model = long_notification();
    model.stacked_actions = true;
    model.actions[0].label = "继续启动并查看已经修复的全部语言资源文件".repeat(2);
    let first = layout(area, &model);
    assert!(first.actions[0].area.height >= 2);
    assert_actions_accessible(&first);
    model.scroll_offset = first.max_scroll_offset;
    let last = layout(area, &model);
    assert_eq!(first.actions, last.actions);
    let terminal = draw(area, &model, &TundraTheme::default_dark());
    assert!(text(&terminal, last.message).contains("最后一行"));
    assert!(text(&terminal, last.actions[1].area).contains("退出"));
}

#[test]
fn too_small_is_reserved_for_required_chrome_and_actions_not_message_length() {
    let model = long_notification();
    let tight = layout(Rect::new(0, 0, 50, 5), &model);
    assert!(tight.message.height >= 1);
    assert_actions_accessible(&tight);
    assert!(matches!(
        notification_layout(Rect::new(0, 0, 50, 4), &model),
        NotificationLayout::TooSmall { .. }
    ));
    assert!(matches!(
        notification_layout(Rect::new(0, 0, 2, 12), &model),
        NotificationLayout::TooSmall { .. }
    ));
    let mut no_actions = model;
    no_actions.actions.clear();
    let tiny = layout(Rect::new(0, 0, 50, 3), &no_actions);
    assert_eq!(tiny.message.height, 1);
    assert!(tiny.scrollbar.is_some());
    assert!(tiny.actions.is_empty());
}
