use super::*;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use workspace_protocol::{AppStartupTimings, Image, LayoutGeneration, Rect, Size};

fn frame(sequence: u64) -> WindowFrame {
    let mut frame = WindowFrame::new(LayoutGeneration(sequence), Size::new(1, 1));
    frame.sequence = sequence;
    frame.processed_input_serial = sequence;
    frame
}

#[test]
fn coalescing_preserves_startup_and_keeps_fence_before_the_replacing_frame() {
    let queue = Queue::new(1024);
    let mut first = frame(1);
    first.startup_timings = Some(AppStartupTimings {
        first_draw_us: 37,
        ..Default::default()
    });
    queue.push(AppToShell::Frame(first)).unwrap();
    queue.push(AppToShell::InputFence { serial: 2 }).unwrap();
    queue.push(AppToShell::Frame(frame(2))).unwrap();
    let state = queue.state.lock().unwrap();
    assert_eq!(state.messages.len(), 2);
    assert_eq!(
        state.messages.front(),
        Some(&AppToShell::InputFence { serial: 2 })
    );
    let last = state.messages.back().unwrap().frame().unwrap();
    assert_eq!(last.sequence, 2);
    assert_eq!(last.processed_input_serial, 2);
    assert_eq!(last.startup_timings.unwrap().first_draw_us, 37);
}

#[test]
fn an_explicit_debug_request_survives_backpressure_and_precedes_close() {
    let queue = Queue::new(MAX_CONTROL_BYTES);
    for _ in 0..MAX_MESSAGES - RESERVED_MESSAGES {
        queue.push(AppToShell::ToggleTasks).unwrap();
    }
    let debug = AppToShell::ShellAction(workspace_protocol::ShellAction::DebugPanicRequested);
    let (error, retained) = queue.push(debug.clone()).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
    assert_eq!(retained, debug);
    for message in [retained, AppToShell::CloseStarted, AppToShell::Closed] {
        assert_eq!(
            event_loop::runtime().block_on(queue.next()).unwrap(),
            AppToShell::ToggleTasks
        );
        queue.complete();
        queue.push(message).unwrap();
    }
    queue.push(AppToShell::Frame(frame(1))).unwrap();
    queue.push(AppToShell::Frame(frame(2))).unwrap();
    let state = queue.state.lock().unwrap();
    let index = |target: &AppToShell| {
        state
            .messages
            .iter()
            .position(|message| message == target)
            .unwrap()
    };
    assert!(index(&debug) < index(&AppToShell::CloseStarted));
    assert!(index(&AppToShell::CloseStarted) < index(&AppToShell::Closed));
}

#[test]
fn backpressure_and_frame_replacement_keep_persisted_control_before_its_confirmation() {
    let queue = Queue::new(MAX_CONTROL_BYTES);
    queue.push(AppToShell::Frame(frame(1))).unwrap();
    for _ in 0..MAX_MESSAGES - RESERVED_MESSAGES {
        queue.push(AppToShell::ToggleTasks).unwrap();
    }
    let commit = AppToShell::ShellAction(workspace_protocol::ShellAction::AutomationChanged(
        workspace_protocol::AutomationCommit {
            enabled: false,
            revision: 8,
            authorization_revision: 7,
            authorization_session: None,
        },
    ));
    assert_eq!(
        queue.push(commit.clone()).unwrap_err().0.kind(),
        io::ErrorKind::WouldBlock
    );
    assert!(matches!(
        event_loop::runtime().block_on(queue.next()).unwrap(),
        AppToShell::Frame(_)
    ));
    queue.complete();
    assert_eq!(
        event_loop::runtime().block_on(queue.next()).unwrap(),
        AppToShell::ToggleTasks
    );
    queue.complete();
    queue.push(commit.clone()).unwrap();
    queue.push(AppToShell::Frame(frame(2))).unwrap();
    let mut confirmed = frame(3);
    confirmed.confirmed_input_fence = 42;
    queue.push(AppToShell::Frame(confirmed)).unwrap();
    let state = queue.state.lock().unwrap();
    let commit_index = state
        .messages
        .iter()
        .position(|message| message == &commit)
        .unwrap();
    let frame_index = state
        .messages
        .iter()
        .position(|message| message.frame().is_some())
        .unwrap();
    assert!(commit_index < frame_index);
    assert_eq!(
        state.messages[frame_index]
            .frame()
            .unwrap()
            .confirmed_input_fence,
        42
    );
}

#[test]
fn byte_backpressure_recovers_after_the_writer_consumes_a_message() {
    let message = ShellToApp::TaskAction {
        id: "x".repeat(1024),
        cancel: false,
    };
    let queue = Queue::new(message.resident_bytes() * 2 - 1);
    queue.push(message.clone()).unwrap();
    let (error, rejected) = queue.push(message.clone()).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
    assert_eq!(rejected, message);
    assert_eq!(
        event_loop::runtime().block_on(queue.next()).unwrap(),
        message
    );
    queue.complete();
    queue.push(message).unwrap();
}

#[test]
fn a_full_input_queue_keeps_fifo_space_for_pointer_cancel_and_close() {
    let queue = Queue::new(MAX_CONTROL_BYTES);
    for serial in 0..(MAX_MESSAGES - RESERVED_MESSAGES) as u64 {
        queue
            .push(ShellToApp::Input {
                serial,
                layout: LayoutGeneration(1),
                event: serde_json::Value::Null,
            })
            .unwrap();
    }
    assert_eq!(
        queue
            .push(ShellToApp::Input {
                serial: 999,
                layout: LayoutGeneration(1),
                event: serde_json::Value::Null
            })
            .unwrap_err()
            .0
            .kind(),
        io::ErrorKind::WouldBlock
    );
    queue
        .push(ShellToApp::CancelPointer {
            layout: LayoutGeneration(1),
        })
        .unwrap();
    queue.push(ShellToApp::RequestClose).unwrap();
    let state = queue.state.lock().unwrap();
    for (index, message) in state
        .messages
        .iter()
        .take(MAX_MESSAGES - RESERVED_MESSAGES)
        .enumerate()
    {
        assert!(matches!(message, ShellToApp::Input { serial, .. } if *serial == index as u64));
    }
    assert!(matches!(
        state.messages.back(),
        Some(ShellToApp::RequestClose)
    ));
}

#[cfg(unix)]
#[test]
#[cfg_attr(not(feature = "native-tests"), ignore = "requires a Unix socket")]
fn newer_frame_resolves_cells_and_images_before_coalescing_older_frames() {
    use platform::shell_control::AsyncLocalListener;
    let path = std::env::temp_dir().join(format!(
        "tundra-frame-{}-{}.sock",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let listener = {
        let _runtime = event_loop::runtime().enter();
        AsyncLocalListener::bind(path.to_str().unwrap()).unwrap()
    };
    let (mut shell, mut host) = event_loop::runtime().block_on(async {
        let client = AsyncLocalConnection::connect(path.to_str().unwrap(), std::process::id())
            .await
            .unwrap();
        let peer = listener.accept().await.unwrap();
        (
            Connection::<ShellToApp, AppToShell>::new(
                client,
                workspace_protocol::MAX_WINDOW_FRAME_BYTES,
            ),
            Connection::<AppToShell, ShellToApp>::new(
                peer,
                workspace_protocol::MAX_WINDOW_FRAME_BYTES,
            ),
        )
    });
    let mut first = WindowFrame::new(LayoutGeneration(1), Size::new(106, 16));
    first.sequence = 1;
    first.startup_timings = Some(AppStartupTimings::default());
    first.images.push(Image {
        image_id: 91,
        preview_id: None,
        width: 1,
        height: 1,
        rgba: Arc::from([3, 5, 7, 255]),
        source: [0, 0, 1, 1],
        padding: [0; 4],
        cell_size: [1, 1],
        area: Rect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        },
        visible: vec![Rect {
            x: 0,
            y: 0,
            width: 1,
            height: 1,
        }],
        z_index: 0,
    });
    host.queue_owned(AppToShell::Frame(first.clone())).unwrap();
    wait_until(|| !host.pending_write());
    first.sequence = 2;
    first.layout = LayoutGeneration(2);
    first.startup_timings = None;
    host.queue_owned(AppToShell::Frame(first.clone())).unwrap();
    wait_until(|| !host.pending_write());
    first.sequence = 3;
    let cells = Arc::make_mut(&mut first.cells);
    cells[31].symbol = "中".into();
    cells[32].skip = true;
    host.queue_owned(AppToShell::Frame(first.clone())).unwrap();
    wait_until(|| !host.pending_write());
    first.sequence = 4;
    first.processed_input_serial = 9;
    first.confirmed_input_fence = 8;
    Arc::make_mut(&mut first.cells)[106].symbol = "e\u{301}".into();
    host.queue_owned(AppToShell::Frame(first.clone())).unwrap();
    wait_until(|| {
        shell
            .incoming
            .state
            .lock()
            .unwrap()
            .messages
            .iter()
            .any(|message| message.frame().is_some_and(|frame| frame.sequence == 4))
    });
    let messages = shell.poll().unwrap();
    assert_eq!(messages.len(), 1);
    let latest = messages[0].frame().unwrap();
    assert_eq!(latest.layout, LayoutGeneration(2));
    assert_eq!(latest.cells, first.cells);
    assert_eq!(latest.processed_input_serial, 9);
    assert_eq!(latest.confirmed_input_fence, 8);
    assert_eq!(&*latest.images[0].rgba, &[3, 5, 7, 255]);
    assert!(latest.startup_timings.is_some());
    drop(host);
    drop(shell);
    drop(listener);
}

fn wait_until(mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let generation = event_loop::generation();
        if predicate() {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "background transport did not make progress"
        );
        event_loop::wait_since(
            generation,
            deadline.saturating_duration_since(Instant::now()),
        );
    }
}
