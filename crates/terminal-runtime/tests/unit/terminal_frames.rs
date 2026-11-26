use super::*;
use crossterm::cursor::SetCursorStyle;
use ratatui::backend::Backend;
use ratatui::layout::Rect;
use ratatui::{TerminalOptions, Viewport};

#[derive(Default)]
struct RecordingWriter {
    writes: Vec<Vec<u8>>,
    flushes: usize,
}

impl Write for RecordingWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.writes.push(bytes.into());
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.flushes += 1;
        Ok(())
    }
}

fn test_terminal() -> HostTerminal<RecordingWriter> {
    let writer = HostTerminalWriter {
        output: RecordingWriter::default(),
        frame: Vec::new(),
        buffering: false,
    };
    Terminal::with_options(
        CrosstermBackend::new(writer),
        TerminalOptions {
            viewport: Viewport::Fixed(Rect::new(0, 0, 12, 4)),
        },
    )
    .unwrap()
}

#[test]
fn screen_clear_cells_image_cleanup_and_cursor_reach_the_host_in_one_write() {
    let mut terminal = test_terminal();
    let mut update = SynchronizedFrame::begin(&mut terminal).unwrap();
    let screen = update.terminal_mut();
    screen.backend_mut().clear().unwrap();
    screen
        .draw(|frame| {
            frame.render_widget("contents", frame.area());
            frame.set_cursor_position((1, 1));
        })
        .unwrap();
    let writer = screen.backend_mut();
    writer.write_all(b"\x1b_Ga=d,d=i,i=2\x1b\\").unwrap();
    Write::flush(writer).unwrap();
    execute!(writer, SetCursorStyle::SteadyBar).unwrap();
    assert!(writer.writer().output.writes.is_empty());
    assert_eq!(writer.writer().output.flushes, 0);

    update.present().unwrap();

    let writer = &terminal.backend().writer().output;
    let [bytes] = &writer.writes[..] else {
        panic!("expected one write for the whole frame")
    };
    let output = std::str::from_utf8(bytes).unwrap();
    assert!(output.starts_with("\x1b[?2026h\x1b[2J"));
    assert!(output.contains("contents"));
    assert!(output.contains("\x1b[?25h\x1b[2;2H"));
    assert!(output.ends_with("\x1b_Ga=d,d=i,i=2\x1b\\\x1b[6 q\x1b[?2026l"));
    assert_eq!(writer.flushes, 1);
}

#[test]
fn abandoning_a_frame_discards_its_commands_and_restores_direct_output() {
    let mut terminal = test_terminal();
    {
        let mut update = SynchronizedFrame::begin(&mut terminal).unwrap();
        update.terminal_mut().backend_mut().clear().unwrap();
    }
    terminal.backend_mut().write_all(b"prompt").unwrap();
    let writer = &terminal.backend().writer().output;
    assert_eq!(writer.writes, [b"prompt".to_vec()]);
}
