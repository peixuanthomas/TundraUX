use super::*;

#[test]
fn an_input_frame_before_the_following_fence_does_not_confirm_the_batch() {
    let mut fences = InputFences::default();
    fences.observe_input(42);
    assert_eq!(fences.processed, 42);
    assert_eq!(fences.confirmed, 0);
    assert!(!fences.confirm_ready(|_| true));
    assert_eq!(fences.drain_confirmed().count(), 0);

    fences.receive(42, 7);
    assert!(!fences.confirm_ready(|_| false));
    assert_eq!(fences.confirmed, 0);
    assert!(fences.confirm_ready(|token| token == 7));
    assert_eq!(fences.confirmed, 42);
    assert_eq!(fences.drain_confirmed().collect::<Vec<_>>(), [42]);
}

#[test]
fn later_ordinary_input_feedback_does_not_confirm_a_pending_clipboard_write() {
    let mut fences = InputFences::default();
    fences.observe_input(42);
    fences.receive(42, 7);
    fences.observe_input(99);
    assert!(!fences.confirm_ready(|_| false));
    assert_eq!(fences.processed, 99);
    assert_eq!(fences.confirmed, 0);
    assert_eq!(fences.drain_confirmed().count(), 0);

    assert!(fences.confirm_ready(|_| true));
    assert_eq!(fences.processed, 99);
    assert_eq!(fences.confirmed, 42);
}

#[test]
fn a_fence_waits_only_for_the_clipboard_token_captured_when_it_arrived() {
    let mut fences = InputFences::default();
    fences.receive(42, 7);
    fences.receive(43, 8);
    assert!(fences.confirm_ready(|token| token <= 7));
    assert_eq!(fences.confirmed, 42);
    assert_eq!(fences.drain_confirmed().collect::<Vec<_>>(), [42]);
    assert!(!fences.confirm_ready(|token| token <= 7));
    assert_eq!(fences.drain_confirmed().count(), 0);

    assert!(fences.confirm_ready(|token| token <= 8));
    assert_eq!(fences.confirmed, 43);
    assert_eq!(fences.drain_confirmed().collect::<Vec<_>>(), [43]);
}

#[test]
fn hidden_acknowledgments_retain_pending_clipboard_fences_and_drain_once() {
    let mut fences = InputFences::default();
    fences.receive(42, 7);
    assert!(!fences.confirm_ready(|_| false));
    assert_eq!(fences.drain_confirmed().count(), 0);
    assert_eq!(fences.pending.len(), 1);

    assert!(fences.confirm_ready(|_| true));
    assert_eq!(fences.drain_confirmed().collect::<Vec<_>>(), [42]);
    assert_eq!(fences.drain_confirmed().count(), 0);
    assert!(fences.pending.is_empty());
}
