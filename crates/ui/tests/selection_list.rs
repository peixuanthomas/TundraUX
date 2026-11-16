use ratatui::{Terminal, backend::TestBackend, layout::Rect};
use ui::components::{
    ComponentEvent, FormChoices, FormDialog, FormField, InputEvent, Key, SelectionList,
    SelectionListItem,
};
use ui::{RenderContext, automation::FrameCapture};

#[test]
fn grouped_search_activates_the_stable_window_id_after_filtering() {
    let mut list = SelectionList::new(
        "choose",
        vec![
            SelectionListItem::new("old", "Other app").with_group("Minimized"),
            SelectionListItem::new("window.42", "编辑器 e\u{301}")
                .with_hint("Document.txt")
                .with_group("Minimized"),
            SelectionListItem::new("new", "Editor").with_group("Start application"),
        ],
    )
    .titled("Choose an application");
    list.open();
    let area = Rect::new(0, 0, 60, 14);
    list.handle_event(InputEvent::Paste("DOCUMENT".into()), area);
    assert_eq!(list.selected_command().unwrap().id.as_str(), "window.42");
    assert_eq!(
        list.handle_event(InputEvent::key(Key::Enter), area),
        ComponentEvent::Activated("window.42".into())
    );
}

#[test]
fn empty_results_and_disabled_items_cannot_activate_hidden_options() {
    let mut list = SelectionList::new(
        "choose",
        vec![SelectionListItem::new("disabled", "窗口").disabled(true)],
    );
    list.open();
    let area = Rect::new(0, 0, 60, 14);
    assert_eq!(
        list.handle_event(InputEvent::key(Key::Enter), area),
        ComponentEvent::Consumed
    );
    list.handle_event(InputEvent::Paste("missing".into()), area);
    assert_eq!(
        list.handle_event(InputEvent::key(Key::Enter), area),
        ComponentEvent::Consumed
    );
    assert!(list.open);
    let mut terminal = Terminal::new(TestBackend::new(60, 14)).unwrap();
    let mut snapshot = None;
    terminal
        .draw(|frame| {
            let capture = FrameCapture::begin(area, "test");
            list.render_with_context(area, frame.buffer_mut(), &RenderContext::default());
            snapshot = Some(capture.finish(frame.buffer_mut()));
        })
        .unwrap();
    assert!(
        snapshot
            .unwrap()
            .nodes
            .iter()
            .any(|node| node.id.ends_with("choose.query") && node.focused)
    );
    assert!(matches!(
        list.handle_event(InputEvent::key(Key::Escape), area),
        ComponentEvent::Dismissed(_)
    ));
}

#[test]
fn filtered_form_choices_preserve_disabled_state_and_the_original_index() {
    let mut field = FormField::text("choice", "Choice", "Alpha");
    field.choices = vec!["Alpha".into(), "Beta".into(), "Gamma".into()];
    let mut form = FormDialog::new("form", "Select", vec![field]);
    form.choice = Some(FormChoices {
        field: 0,
        values: vec!["Alpha".into(), "Beta".into(), "Gamma".into()],
        disabled: vec![false, false, true],
        ..Default::default()
    });
    let area = Rect::new(0, 0, 108, 20);
    form.handle_event(InputEvent::Paste("Gamma".into()), area);
    let choice = form.choice.as_ref().unwrap();
    assert_eq!(choice.values, vec!["Gamma"]);
    assert_eq!(choice.original_index(0), 2);
    assert!(choice.is_disabled(0));
    form.handle_event(InputEvent::key(Key::Enter), area);
    assert!(form.choice.is_some());
    assert_eq!(form.fields[0].value, "Alpha");
}
