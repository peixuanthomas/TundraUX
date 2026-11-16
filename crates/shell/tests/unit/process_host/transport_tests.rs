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
fn byte_backpressure_recovers_after_the_writer_consumes_a_message() {
    let message = ShellToApp::OpenLauncherEntry {
        id: "x".repeat(1024),
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

#[cfg(unix)]
#[test]
fn newer_layout_frame_resolves_images_from_a_coalesced_older_layout() {
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
    let mut first = frame(1);
    first.startup_timings = Some(AppStartupTimings::default());
    first.images.push(Image {
        image_id: 91,
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
    host.queue_owned(AppToShell::Frame(first)).unwrap();
    wait_until(|| {
        shell
            .incoming
            .state
            .lock()
            .unwrap()
            .messages
            .iter()
            .any(|message| message.frame().is_some_and(|frame| frame.sequence == 2))
    });
    let messages = shell.poll().unwrap();
    assert_eq!(messages.len(), 1);
    let latest = messages[0].frame().unwrap();
    assert_eq!(latest.layout, LayoutGeneration(2));
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
