use super::*;
use crate::{CommandLineCell, TerminalGraphicsProbe};
use ratatui::{Terminal, backend::TestBackend, style::Color};

fn fragment() -> TerminalImageFragment {
    TerminalImageFragment {
        image_id: 42,
        width: 4,
        height: 2,
        rgba: Arc::from([
            255, 0, 0, 255, 255, 0, 0, 255, 0, 255, 0, 255, 0, 255, 0, 255, 255, 0, 0, 255, 255, 0,
            0, 255, 0, 255, 0, 255, 0, 255, 0, 255,
        ]),
        source: [0, 0, 4, 2],
        padding: [0; 4],
        cell_size: [2, 2],
        area: Rect::new(1, 1, 2, 1),
        z_index: 0,
    }
}

fn picker(protocol: EditorGraphicsProtocol, tmux: bool) -> EditorImagePicker {
    TerminalGraphicsProbe::from_terminal_capabilities(protocol, 8, 16, tmux, false)
        .picker()
        .unwrap()
        .clone()
}

fn kitty_packets(symbol: &str, tmux: bool) -> (String, Vec<u8>) {
    let (_, escape, _) = ratatui_image::picker::cap_parser::Parser::tmux_start_escape_end(tmux);
    let start = format!("{escape}_G");
    let end = format!("{escape}\\");
    let chunks = symbol
        .split(&start)
        .skip(1)
        .map(|chunk| chunk.split_once(&end).unwrap().0.split_once(';').unwrap())
        .collect::<Vec<_>>();
    let mut decoded = Vec::new();
    for (index, (controls, payload)) in chunks.iter().enumerate() {
        assert!(payload.len() <= 4096 && payload.len() % 4 == 0);
        assert!(controls.ends_with(if index + 1 == chunks.len() {
            "m=0"
        } else {
            "m=1"
        }));
        decoded.extend(base64_simd::STANDARD.decode_to_vec(payload).unwrap());
    }
    (chunks[0].0.to_owned(), decoded)
}

#[test]
fn workspace_kitty_compresses_a_large_fitted_page_and_clears_it_after_hiding() {
    use std::io::Read as _;
    let area = workspace_protocol::Rect {
        x: 24,
        y: 2,
        width: 152,
        height: 98,
    };
    let width = u32::from(area.width) * 8;
    let height = u32::from(area.height) * 16;
    let image = workspace_protocol::Image {
        image_id: 42,
        width,
        height,
        rgba: Arc::from([162, 40, 20, 255].repeat((width * height) as usize)),
        source: [0, 0, width, height],
        padding: [0; 4],
        cell_size: [8, 16],
        area,
        visible: vec![area],
        z_index: 0,
    };

    for tmux in [false, true] {
        let picker = picker(EditorGraphicsProtocol::Kitty, tmux);
        let mut screen = Terminal::new(TestBackend::new(220, 104)).unwrap();
        let mut cache = TerminalImageCache::default();
        screen
            .draw(|frame| {
                cache.render_workspace(frame, frame.area(), std::slice::from_ref(&image), &picker)
            })
            .unwrap();
        let symbol = screen.backend().buffer()[(area.x, area.y)].symbol();
        let (controls, compressed) = kitty_packets(symbol, tmux);
        assert!(controls.contains("a=T,U=1,f=32,t=d,s=1216,v=1568,c=152,r=98,o=z,"));
        assert!(symbol.contains('\u{10eeee}'));
        assert!(compressed.len() < image.rgba.len() / 50);
        let mut pixels = Vec::new();
        flate2::read::ZlibDecoder::new(compressed.as_slice())
            .read_to_end(&mut pixels)
            .unwrap();
        assert_eq!(pixels.as_slice(), image.rgba.as_ref());

        let id = cache.entries.values().next().unwrap().kitty.unwrap().0;
        screen
            .draw(|frame| {
                cache.render_workspace(frame, frame.area(), std::slice::from_ref(&image), &picker)
            })
            .unwrap();
        assert!(
            !screen.backend().buffer()[(area.x, area.y)]
                .symbol()
                .contains("_G")
        );
        assert!(cache.take_cleanup().is_empty());
        screen
            .draw(|frame| cache.render_workspace(frame, frame.area(), &[], &picker))
            .unwrap();
        assert_eq!(screen.backend().buffer()[(area.x, area.y)].symbol(), " ");
        let (start, escape, end) =
            ratatui_image::picker::cap_parser::Parser::tmux_start_escape_end(tmux);
        assert_eq!(
            cache.take_cleanup(),
            format!("{start}{escape}_Ga=d,d=I,i={id}{escape}\\{end}").into_bytes()
        );
    }
}

#[test]
fn workspace_kitty_keeps_raw_uploads_when_compression_saves_little() {
    let area = workspace_protocol::Rect {
        x: 1,
        y: 1,
        width: 20,
        height: 16,
    };
    let width = u32::from(area.width) * 8;
    let height = u32::from(area.height) * 16;
    let mut noise = 0x8bd1ff45u32;
    let pixels = (0..width * height)
        .flat_map(|_| {
            noise ^= noise << 13;
            noise ^= noise >> 17;
            noise ^= noise << 5;
            let [red, green, blue, _] = noise.to_le_bytes();
            [red, green, blue, 255]
        })
        .collect::<Vec<_>>();
    let image = workspace_protocol::Image {
        image_id: 42,
        width,
        height,
        rgba: Arc::from(pixels),
        source: [0, 0, width, height],
        padding: [0; 4],
        cell_size: [8, 16],
        area,
        visible: vec![area],
        z_index: 0,
    };
    let picker = picker(EditorGraphicsProtocol::Kitty, false);
    let mut screen = Terminal::new(TestBackend::new(24, 20)).unwrap();
    let mut cache = TerminalImageCache::default();
    screen
        .draw(|frame| {
            cache.render_workspace(frame, frame.area(), std::slice::from_ref(&image), &picker)
        })
        .unwrap();
    let symbol = screen.backend().buffer()[(area.x, area.y)].symbol();
    let (controls, pixels) = kitty_packets(symbol, false);
    assert!(controls.contains("a=T,U=1,f=32,t=d,"));
    assert!(!controls.contains("o=z"));
    assert_eq!(pixels.as_slice(), image.rgba.as_ref());
}

#[test]
fn workspace_iterm2_keeps_a_retina_preview_when_the_pane_expands_or_is_clipped() {
    let [width, height] = [1187, 1536];
    let full = workspace_protocol::Rect {
        x: 20,
        y: 1,
        width: 85,
        height: 48,
    };
    let split = workspace_protocol::Rect {
        width: 43,
        height: 24,
        ..full
    };
    let mut rgba = [162, 40, 20, 255].repeat((width * height / 2) as usize);
    rgba.extend([20, 40, 162, 255].repeat((width * height / 2) as usize));
    let mut image = workspace_protocol::Image {
        image_id: 42,
        width,
        height,
        rgba: Arc::from(rgba),
        source: [0, 0, width, height],
        padding: [0; 4],
        cell_size: [14, 32],
        area: split,
        visible: vec![split],
        z_index: 0,
    };
    let graphical = TerminalGraphicsProbe::from_terminal_capabilities(
        EditorGraphicsProtocol::Iterm2,
        14,
        32,
        false,
        false,
    );
    let picker = graphical.picker().unwrap();
    let mut screen = Terminal::new(TestBackend::new(208, 55)).unwrap();
    let mut cache = TerminalImageCache::default();
    for (area, viewport_height, source_y, rows, png_size, last_pixel) in [
        (split, 26, 0, 24, [602, 768], [20, 40, 162, 255]),
        (full, 55, 0, 48, [1187, 1536], [20, 40, 162, 255]),
        (full, 25, 0, 24, [1187, 768], [162, 40, 20, 255]),
        (
            workspace_protocol::Rect { height: 24, ..full },
            25,
            768,
            24,
            [1187, 768],
            [20, 40, 162, 255],
        ),
    ] {
        image.area = area;
        image.visible[0] = area;
        image.source = [0, source_y, width, height - source_y];
        screen
            .draw(|frame| {
                frame.buffer_mut()[(full.x, full.y)].set_symbol("▀");
                let viewport = Rect::new(0, 0, frame.area().width, viewport_height);
                cache.render_workspace(frame, viewport, std::slice::from_ref(&image), picker)
            })
            .unwrap();
        let symbol = screen.backend().buffer()[(full.x, full.y)].symbol();
        let (_, packet) = symbol
            .split_once("\x1b]1337;File=inline=1;")
            .unwrap_or_else(|| {
                panic!("no native picture for {area:?} at height {viewport_height}")
            });
        let (controls, payload) = packet.split_once(':').unwrap();
        assert!(controls.contains(&format!("width={};height={rows};", area.width)));
        let encoded = base64_simd::STANDARD
            .decode_to_vec(payload.strip_suffix('\x07').unwrap())
            .unwrap();
        let pixels = image::load_from_memory_with_format(&encoded, image::ImageFormat::Png)
            .unwrap()
            .into_rgba8();
        assert_eq!([pixels.width(), pixels.height()], png_size);
        assert_eq!(pixels.get_pixel(0, pixels.height() - 1).0, last_pixel);
    }
}

#[test]
fn fallback_crops_pixels_to_the_visible_terminal_and_applies_the_host_offset() {
    let mut snapshot = CommandLineTerminalSnapshot::blank(4, 2);
    snapshot.images.push(fragment());
    let mut cache = TerminalImageCache::default();
    let mut screen = Terminal::new(TestBackend::new(12, 8)).unwrap();
    screen
        .draw(|frame| cache.render(frame, Rect::new(5, 3, 2, 2), &snapshot, None))
        .unwrap();
    let buffer = screen.backend().buffer();
    assert_eq!(buffer[(6, 4)].bg, Color::Rgb(255, 0, 0));
    assert_eq!(buffer[(7, 4)].bg, Color::Reset);
    assert_eq!(buffer[(5, 4)].bg, Color::Reset);
    assert_eq!(buffer[(6, 3)].bg, Color::Reset);
}

#[test]
fn background_images_leave_text_and_wide_continuations_visible() {
    let mut snapshot = CommandLineTerminalSnapshot::blank(4, 2);
    let mut image = fragment();
    image.area = Rect::new(0, 0, 4, 2);
    image.z_index = -1;
    snapshot.images.push(image);
    snapshot.set_cell(
        1,
        0,
        CommandLineCell {
            symbol: "界".into(),
            ..Default::default()
        },
    );
    let mut cache = TerminalImageCache::default();
    let mut screen = Terminal::new(TestBackend::new(4, 2)).unwrap();
    screen
        .draw(|frame| {
            frame.buffer_mut()[(1, 0)].set_symbol("界");
            cache.render(frame, frame.area(), &snapshot, None);
        })
        .unwrap();
    let buffer = screen.backend().buffer();
    assert_eq!(buffer[(1, 0)].symbol(), "界");
    assert_eq!(buffer[(2, 0)].bg, Color::Reset);
    assert_ne!(buffer[(0, 1)].bg, Color::Reset);
}

#[test]
fn native_backends_encode_only_the_clipped_picture_area() {
    for (protocol, prefix) in [
        (EditorGraphicsProtocol::Kitty, "\x1b_G"),
        (EditorGraphicsProtocol::Sixel, "\x1bP"),
        (EditorGraphicsProtocol::Iterm2, "\x1b]1337;File=inline=1"),
    ] {
        let mut snapshot = CommandLineTerminalSnapshot::blank(4, 2);
        snapshot.images.push(fragment());
        let mut cache = TerminalImageCache::default();
        let mut screen = Terminal::new(TestBackend::new(12, 8)).unwrap();
        screen
            .draw(|frame| {
                cache.render(
                    frame,
                    Rect::new(5, 3, 2, 2),
                    &snapshot,
                    Some(&picker(protocol, false)),
                )
            })
            .unwrap();
        let buffer = screen.backend().buffer();
        assert!(buffer[(6, 4)].symbol().contains(prefix), "{protocol:?}");
        assert!(!buffer[(7, 4)].symbol().contains('\x1b'));
        assert!(!buffer[(5, 4)].symbol().contains('\x1b'));
    }
}

#[test]
fn workspace_round_trip_clips_native_picture_and_cleans_up_kitty() {
    use workspace_protocol::transport::Framed;

    let mut source = CommandLineTerminalSnapshot::blank(4, 2);
    source.images.push(fragment());
    let mut worker_cache = TerminalImageCache::default();
    let mut worker = Terminal::new(TestBackend::new(10, 5)).unwrap();
    worker_cache.capture_workspace(Rect::new(2, 1, 4, 3));
    worker
        .draw(|frame| {
            worker_cache.render(frame, Rect::new(2, 1, 2, 2), &source, None);
        })
        .unwrap();
    let images = worker_cache.take_workspace_images();
    assert_eq!(worker.backend().buffer()[(3, 2)].bg, Color::Rgb(255, 0, 0));
    assert_eq!(images[0].area.width, 2);
    assert_eq!(images[0].visible[0].width, 1);
    #[cfg(unix)]
    use std::os::unix::net::UnixStream;
    #[cfg(unix)]
    let (sender, receiver) = UnixStream::pair().unwrap();
    #[cfg(windows)]
    let (sender, receiver) = {
        use std::net::{TcpListener, TcpStream};
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let sender = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        (sender, listener.accept().unwrap().0)
    };
    sender.set_nonblocking(true).unwrap();
    receiver.set_nonblocking(true).unwrap();
    let mut sender = Framed::new(sender, 4096);
    let mut receiver = Framed::new(receiver, 4096);
    sender.queue(&images).unwrap();
    sender.poll::<Vec<workspace_protocol::Image>>().unwrap();
    let mut images = receiver.poll::<Vec<workspace_protocol::Image>>().unwrap();
    let images = images.pop().unwrap();

    for (protocol, prefix) in [
        (EditorGraphicsProtocol::Kitty, "\x1b_G"),
        (EditorGraphicsProtocol::Sixel, "\x1bP"),
        (EditorGraphicsProtocol::Iterm2, "\x1b]1337;File=inline=1"),
    ] {
        let mut shell_cache = TerminalImageCache::default();
        let mut shell = Terminal::new(TestBackend::new(12, 7)).unwrap();
        let pane = Rect::new(5, 3, 4, 3);
        let picker = picker(protocol, false);
        shell
            .draw(|frame| {
                frame.buffer_mut()[(7, 4)].set_symbol("X");
                shell_cache.render_workspace(frame, pane, &images, &picker);
            })
            .unwrap();
        assert!(
            shell.backend().buffer()[(6, 4)].symbol().contains(prefix),
            "{protocol:?}"
        );
        assert_eq!(shell.backend().buffer()[(7, 4)].symbol(), "X");

        if protocol == EditorGraphicsProtocol::Kitty {
            let id = shell_cache
                .entries
                .values()
                .next()
                .unwrap()
                .kitty
                .unwrap()
                .0;
            shell
                .draw(|frame| shell_cache.render_workspace(frame, pane, &images, &picker))
                .unwrap();
            assert!(shell_cache.take_cleanup().is_empty());
            shell
                .draw(|frame| {
                    shell_cache.render_workspace(frame, Rect::new(6, 3, 4, 3), &images, &picker);
                })
                .unwrap();
            assert_eq!(
                shell_cache.take_cleanup(),
                format!("\x1b_Ga=d,d=I,i={id}\x1b\\").as_bytes()
            );
            let next = shell_cache
                .entries
                .values()
                .next()
                .unwrap()
                .kitty
                .unwrap()
                .0;
            assert_ne!(next, id);
            shell_cache.clear();
            assert_eq!(
                shell_cache.take_cleanup(),
                format!("\x1b_Ga=d,d=I,i={next}\x1b\\").as_bytes()
            );
        }
    }
}

#[test]
fn workspace_captures_only_native_safe_images() {
    let mut source = CommandLineTerminalSnapshot::blank(4, 2);
    let mut image = fragment();
    Arc::make_mut(&mut image.rgba)[3] = 0;
    source.images.push(image);
    let mut cache = TerminalImageCache::default();
    let mut screen = Terminal::new(TestBackend::new(6, 4)).unwrap();
    let size = Rect::new(0, 0, 4, 2);
    cache.capture_workspace(size);
    screen
        .draw(|frame| cache.render(frame, size, &source, None))
        .unwrap();
    assert!(cache.take_workspace_images().is_empty());
    assert_ne!(screen.backend().buffer()[(1, 1)].bg, Color::Reset);
}

#[test]
fn clearing_or_replacing_a_picture_retires_only_its_owned_native_resource() {
    let mut snapshot = CommandLineTerminalSnapshot::blank(4, 2);
    snapshot.images.push(fragment());
    let picker = picker(EditorGraphicsProtocol::Kitty, false);
    let mut cache = TerminalImageCache::default();
    let mut screen = Terminal::new(TestBackend::new(4, 2)).unwrap();
    screen
        .draw(|frame| cache.render(frame, frame.area(), &snapshot, Some(&picker)))
        .unwrap();
    let old = cache.entries.values().next().unwrap().kitty.unwrap().0;
    screen
        .draw(|frame| cache.render(frame, frame.area(), &snapshot, Some(&picker)))
        .unwrap();
    assert!(cache.take_cleanup().is_empty());
    snapshot.images[0].rgba = Arc::from([255_u8; 32]);
    screen
        .draw(|frame| cache.render(frame, frame.area(), &snapshot, Some(&picker)))
        .unwrap();
    let new = cache.entries.values().next().unwrap().kitty.unwrap().0;
    assert_ne!(old, new);
    assert_eq!(
        cache.take_cleanup(),
        format!("\x1b_Ga=d,d=I,i={old}\x1b\\").as_bytes()
    );
    snapshot.images.clear();
    screen
        .draw(|frame| cache.render(frame, frame.area(), &snapshot, Some(&picker)))
        .unwrap();
    assert!(cache.entries.is_empty());
    assert_eq!(
        cache.take_cleanup(),
        format!("\x1b_Ga=d,d=I,i={new}\x1b\\").as_bytes()
    );
    assert_eq!(screen.backend().buffer()[(1, 1)].symbol(), " ");
}

#[test]
fn tmux_cleanup_wraps_only_generated_resource_identifiers() {
    let mut cleanup = Vec::new();
    append_cleanup(&mut cleanup, Some((19, true)));
    assert_eq!(cleanup, b"\x1bPtmux;\x1b\x1b_Ga=d,d=I,i=19\x1b\x1b\\\x1b\\");
}

#[test]
fn malformed_or_oversized_picture_data_does_not_change_the_screen() {
    for image in [
        TerminalImageFragment {
            rgba: Arc::from([0_u8; 4]),
            ..fragment()
        },
        TerminalImageFragment {
            source: [4, 0, 1, 1],
            ..fragment()
        },
        TerminalImageFragment {
            width: u32::MAX,
            height: u32::MAX,
            ..fragment()
        },
    ] {
        let mut snapshot = CommandLineTerminalSnapshot::blank(4, 2);
        snapshot.images.push(image);
        let mut cache = TerminalImageCache::default();
        let mut screen = Terminal::new(TestBackend::new(4, 2)).unwrap();
        screen
            .draw(|frame| cache.render(frame, frame.area(), &snapshot, None))
            .unwrap();
        assert!(cache.entries.is_empty());
        assert!(
            screen
                .backend()
                .buffer()
                .content()
                .iter()
                .all(|cell| cell.symbol() == " ")
        );
    }
}

fn offset_picture() -> TerminalImageFragment {
    TerminalImageFragment {
        image_id: 100,
        width: 17,
        height: 6,
        rgba: Arc::from([255, 0, 0, 255].repeat(17 * 6)),
        source: [0, 0, 17, 6],
        padding: [3, 4, 4, 6],
        cell_size: [8, 16],
        area: Rect::new(0, 0, 3, 1),
        z_index: 0,
    }
}

#[test]
fn native_pixel_canvas_preserves_non_cell_size_and_xy_offsets() {
    let image = offset_picture();
    let canvas = image_canvas(&image).unwrap();
    assert_eq!(canvas.dimensions(), (24, 16));
    assert_eq!(canvas.get_pixel(3, 4).0, [255, 0, 0, 255]);
    assert_eq!(canvas.get_pixel(19, 9).0, [255, 0, 0, 255]);
    for (x, y) in [(2, 4), (3, 3), (20, 4), (3, 10), (23, 15)] {
        assert_eq!(canvas.get_pixel(x, y)[3], 0, "{x},{y}");
    }
    assert_eq!(
        source_for_area(&image, Rect::new(1, 0, 1, 1)),
        Some([8, 0, 8, 16])
    );
    let clipped = image::imageops::crop_imm(&canvas, 8, 0, 8, 16).to_image();
    assert_eq!(clipped.get_pixel(0, 4).0, [255, 0, 0, 255]);
    assert_eq!(clipped.get_pixel(7, 9).0, [255, 0, 0, 255]);
    assert_eq!(clipped.get_pixel(0, 10)[3], 0);
}

#[test]
fn viewport_and_scrollback_crop_the_padded_canvas_without_rescaling_it() {
    let image = TerminalImageFragment {
        height: 20,
        rgba: Arc::from([255, 0, 0, 255].repeat(17 * 20)),
        source: [0, 0, 17, 20],
        padding: [3, 4, 4, 8],
        area: Rect::new(0, 0, 3, 2),
        ..offset_picture()
    };
    let canvas = image_canvas(&image).unwrap();
    assert_eq!(
        source_for_area(&image, Rect::new(1, 1, 2, 1)),
        Some([8, 16, 16, 16])
    );
    let cropped = image::imageops::crop_imm(&canvas, 8, 16, 16, 16).to_image();
    assert_eq!(cropped.get_pixel(11, 7).0, [255, 0, 0, 255]);
    assert_eq!(cropped.get_pixel(12, 7)[3], 0);
    assert_eq!(cropped.get_pixel(0, 8)[3], 0);
}

#[test]
fn fallback_composites_alpha_over_the_existing_background_and_preserves_empty_pixels() {
    let mut snapshot = CommandLineTerminalSnapshot::blank(1, 1);
    snapshot.images.push(TerminalImageFragment {
        width: 2,
        height: 2,
        rgba: Arc::from([255, 0, 0, 128].repeat(4)),
        source: [0, 0, 2, 2],
        area: Rect::new(0, 0, 1, 1),
        ..fragment()
    });
    let mut cache = TerminalImageCache::default();
    let mut screen = Terminal::new(TestBackend::new(1, 1)).unwrap();
    screen
        .draw(|frame| {
            frame.buffer_mut()[(0, 0)].set_bg(Color::Rgb(0, 0, 255));
            cache.render(frame, frame.area(), &snapshot, None);
        })
        .unwrap();
    let Color::Rgb(red, green, blue) = screen.backend().buffer()[(0, 0)].bg else {
        panic!("RGB fallback")
    };
    assert!((127..=128).contains(&red));
    assert_eq!(green, 0);
    assert!((126..=128).contains(&blue));
    snapshot.images[0].rgba = Arc::from([255, 0, 0, 0].repeat(4));
    screen
        .draw(|frame| {
            frame.buffer_mut()[(0, 0)]
                .set_symbol("X")
                .set_bg(Color::Rgb(4, 5, 6));
            cache.render(frame, frame.area(), &snapshot, None);
        })
        .unwrap();
    assert_eq!(screen.backend().buffer()[(0, 0)].symbol(), "X");
    assert_eq!(screen.backend().buffer()[(0, 0)].bg, Color::Rgb(4, 5, 6));
}

#[test]
fn fallback_uses_source_cell_geometry_for_partial_edge_cells() {
    let mut snapshot = CommandLineTerminalSnapshot::blank(3, 1);
    snapshot.images.push(TerminalImageFragment {
        height: 16,
        rgba: Arc::from([255, 0, 0, 255].repeat(17 * 16)),
        source: [0, 0, 17, 16],
        padding: [3, 0, 4, 0],
        ..offset_picture()
    });
    let mut cache = TerminalImageCache::default();
    let mut screen = Terminal::new(TestBackend::new(3, 1)).unwrap();
    screen
        .draw(|frame| {
            for x in 0..3 {
                frame.buffer_mut()[(x, 0)].set_bg(Color::Rgb(0, 0, 255));
            }
            cache.render(frame, frame.area(), &snapshot, None);
        })
        .unwrap();
    let buffer = screen.backend().buffer();
    assert_eq!(buffer[(1, 0)].bg, Color::Rgb(255, 0, 0));
    for x in [0, 2] {
        let Color::Rgb(red, green, blue) = buffer[(x, 0)].bg else {
            panic!("RGB fallback")
        };
        assert!(red > 0 && blue > 0);
        assert_eq!(green, 0);
    }
}

#[test]
fn native_transparent_holes_preserve_text_without_protocol_skip_cells() {
    for protocol in [
        EditorGraphicsProtocol::Kitty,
        EditorGraphicsProtocol::Sixel,
        EditorGraphicsProtocol::Iterm2,
    ] {
        let mut snapshot = CommandLineTerminalSnapshot::blank(2, 1);
        snapshot.images.push(TerminalImageFragment {
            width: 2,
            height: 2,
            rgba: Arc::from([0, 0, 0, 0, 255, 0, 0, 255, 0, 0, 0, 0, 255, 0, 0, 255]),
            source: [0, 0, 2, 2],
            cell_size: [1, 2],
            area: Rect::new(0, 0, 2, 1),
            ..fragment()
        });
        let mut cache = TerminalImageCache::default();
        let mut screen = Terminal::new(TestBackend::new(2, 1)).unwrap();
        screen
            .draw(|frame| {
                frame.buffer_mut()[(0, 0)].set_symbol("X");
                cache.render(
                    frame,
                    frame.area(),
                    &snapshot,
                    Some(&picker(protocol, false)),
                );
            })
            .unwrap();
        let buffer = screen.backend().buffer();
        assert_eq!(buffer[(0, 0)].symbol(), "X", "{protocol:?}");
        assert_eq!(
            raster_background(&buffer[(1, 0)]),
            [Color::Rgb(255, 0, 0); 2]
        );
        assert!(
            cache
                .entries
                .values()
                .all(|entry| matches!(entry.protocol, Protocol::Halfblocks(_)))
        );
        assert!(buffer.content().iter().all(|cell| {
            !cell.symbol().contains('\x1b')
                && cell.diff_option != ratatui::buffer::CellDiffOption::Skip
        }));
    }
}

#[test]
fn overlapping_native_images_keep_both_layers_when_their_payload_anchors_coincide() {
    for protocol in [
        EditorGraphicsProtocol::Kitty,
        EditorGraphicsProtocol::Sixel,
        EditorGraphicsProtocol::Iterm2,
    ] {
        let mut snapshot = CommandLineTerminalSnapshot::blank(2, 1);
        snapshot.images.push(TerminalImageFragment {
            width: 2,
            height: 2,
            rgba: Arc::from([255, 0, 0, 255].repeat(4)),
            source: [0, 0, 2, 2],
            cell_size: [1, 2],
            area: Rect::new(0, 0, 2, 1),
            ..fragment()
        });
        snapshot.images.push(TerminalImageFragment {
            image_id: 43,
            width: 1,
            height: 2,
            rgba: Arc::from([0, 0, 255, 255].repeat(2)),
            source: [0, 0, 1, 2],
            cell_size: [1, 2],
            area: Rect::new(0, 0, 1, 1),
            z_index: 1,
            ..fragment()
        });
        let mut cache = TerminalImageCache::default();
        let mut screen = Terminal::new(TestBackend::new(2, 1)).unwrap();
        screen
            .draw(|frame| {
                cache.render(
                    frame,
                    frame.area(),
                    &snapshot,
                    Some(&picker(protocol, false)),
                );
            })
            .unwrap();
        let buffer = screen.backend().buffer();
        assert_eq!(
            raster_background(&buffer[(0, 0)]),
            [Color::Rgb(0, 0, 255); 2]
        );
        assert_eq!(
            raster_background(&buffer[(1, 0)]),
            [Color::Rgb(255, 0, 0); 2]
        );
        assert!(
            buffer
                .content()
                .iter()
                .all(|cell| !cell.symbol().contains('\x1b')),
            "{protocol:?}"
        );
    }
}

#[test]
fn translucent_layers_blend_each_half_cell_and_leave_transparent_lower_pixels_visible() {
    for protocol in [
        EditorGraphicsProtocol::Kitty,
        EditorGraphicsProtocol::Sixel,
        EditorGraphicsProtocol::Iterm2,
    ] {
        let mut snapshot = CommandLineTerminalSnapshot::blank(1, 1);
        snapshot.images.push(TerminalImageFragment {
            width: 1,
            height: 2,
            rgba: Arc::from([255, 0, 0, 255, 0, 0, 255, 255]),
            source: [0, 0, 1, 2],
            cell_size: [1, 2],
            area: Rect::new(0, 0, 1, 1),
            ..fragment()
        });
        snapshot.images.push(TerminalImageFragment {
            image_id: 43,
            rgba: Arc::from([0, 255, 0, 128, 0, 0, 0, 0]),
            z_index: 1,
            ..snapshot.images[0].clone()
        });
        let mut cache = TerminalImageCache::default();
        let mut screen = Terminal::new(TestBackend::new(1, 1)).unwrap();
        screen
            .draw(|frame| {
                cache.render(
                    frame,
                    frame.area(),
                    &snapshot,
                    Some(&picker(protocol, false)),
                );
            })
            .unwrap();
        let [upper, lower] = raster_background(&screen.backend().buffer()[(0, 0)]);
        let Color::Rgb(red, green, blue) = upper else {
            panic!("RGB raster")
        };
        assert!((126..=128).contains(&red), "{protocol:?}: {upper:?}");
        assert!((127..=128).contains(&green));
        assert_eq!(blue, 0);
        assert_eq!(lower, Color::Rgb(0, 0, 255));
    }
}

#[test]
fn preparation_rejects_large_temporary_canvases_before_allocating_or_encoding() {
    let image = TerminalImageFragment {
        width: 1,
        height: 1,
        rgba: Arc::from([255, 0, 0, 255]),
        source: [0, 0, 1, 1],
        cell_size: [1024, 1024],
        area: Rect::new(0, 0, 4, 4),
        ..fragment()
    };
    let picker = picker(EditorGraphicsProtocol::Kitty, false);
    let crop = source_for_area(&image, image.area).unwrap();
    let (stored, working) = preparation_cost(&image, Size::new(4, 4), crop, Some(&picker)).unwrap();
    assert!(stored + working > CACHE_BUDGET);
    let mut snapshot = CommandLineTerminalSnapshot::blank(4, 4);
    snapshot.images.push(image);
    let mut cache = TerminalImageCache::default();
    let mut screen = Terminal::new(TestBackend::new(4, 4)).unwrap();
    screen
        .draw(|frame| {
            cache.render(frame, frame.area(), &snapshot, Some(&picker));
        })
        .unwrap();
    assert!(cache.entries.is_empty());
    assert!(
        screen
            .backend()
            .buffer()
            .content()
            .iter()
            .all(|cell| cell.symbol() == " ")
    );
}

#[test]
fn preparation_bounds_the_float_resize_pass_as_well_as_final_pixels() {
    let image = TerminalImageFragment {
        width: 8192,
        height: 1,
        rgba: Arc::from([255, 0, 0, 255].repeat(8192)),
        source: [0, 0, 8192, 1],
        cell_size: [1, 4096],
        area: Rect::new(0, 0, 1, 1),
        ..fragment()
    };
    let picker = picker(EditorGraphicsProtocol::Kitty, false);
    let crop = source_for_area(&image, image.area).unwrap();
    assert!(preparation_cost(&image, Size::new(1, 1), crop, Some(&picker)).is_none());
    let matching_cell = TerminalGraphicsProbe::from_terminal_capabilities(
        EditorGraphicsProtocol::Kitty,
        1,
        4096,
        false,
        false,
    );
    assert!(preparation_cost(&image, Size::new(1, 1), crop, matching_cell.picker()).is_none());
}

#[test]
fn preparing_a_picture_evicts_unused_cached_resources_before_the_new_allocation() {
    let mut snapshot = CommandLineTerminalSnapshot::blank(4, 2);
    snapshot.images.push(fragment());
    let picker = picker(EditorGraphicsProtocol::Kitty, false);
    let mut cache = TerminalImageCache::default();
    let mut screen = Terminal::new(TestBackend::new(4, 2)).unwrap();
    screen
        .draw(|frame| cache.render(frame, frame.area(), &snapshot, Some(&picker)))
        .unwrap();
    let old = cache.entries.values().next().unwrap().kitty.unwrap().0;
    cache.entries.values_mut().next().unwrap().bytes = CACHE_BUDGET - snapshot.images[0].rgba.len();
    snapshot.images[0].rgba = Arc::from([0, 0, 255, 255].repeat(8));
    screen
        .draw(|frame| cache.render(frame, frame.area(), &snapshot, Some(&picker)))
        .unwrap();
    assert_eq!(cache.entries.len(), 1);
    assert!(cache.cached_bytes() <= CACHE_BUDGET);
    assert_ne!(cache.entries.values().next().unwrap().kitty.unwrap().0, old);
    assert_eq!(
        cache.take_cleanup(),
        format!("\x1b_Ga=d,d=I,i={old}\x1b\\").as_bytes()
    );
}

#[test]
fn pictures_already_used_this_frame_count_against_later_preparation() {
    let mut snapshot = CommandLineTerminalSnapshot::blank(4, 2);
    snapshot.images.push(fragment());
    let picker = picker(EditorGraphicsProtocol::Kitty, false);
    let mut cache = TerminalImageCache::default();
    let mut screen = Terminal::new(TestBackend::new(4, 2)).unwrap();
    screen
        .draw(|frame| cache.render(frame, frame.area(), &snapshot, Some(&picker)))
        .unwrap();
    let old = cache.entries.values().next().unwrap().kitty.unwrap().0;
    cache.entries.values_mut().next().unwrap().bytes = CACHE_BUDGET - snapshot.images[0].rgba.len();
    snapshot.images.push(TerminalImageFragment {
        image_id: 43,
        rgba: Arc::from([0, 0, 255, 255].repeat(8)),
        area: Rect::new(0, 0, 1, 1),
        z_index: 1,
        ..fragment()
    });
    screen
        .draw(|frame| cache.render(frame, frame.area(), &snapshot, Some(&picker)))
        .unwrap();
    assert_eq!(cache.entries.len(), 1);
    assert_eq!(cache.cached_bytes(), CACHE_BUDGET);
    assert_eq!(cache.entries.values().next().unwrap().kitty.unwrap().0, old);
    assert!(cache.take_cleanup().is_empty());
    assert_eq!(screen.backend().buffer()[(0, 0)].symbol(), " ");
}

#[test]
fn raster_images_remove_inherited_skip_and_inverse_styles() {
    let mut snapshot = CommandLineTerminalSnapshot::blank(4, 2);
    snapshot.images.push(TerminalImageFragment {
        rgba: Arc::from([255, 0, 0, 255].repeat(8)),
        ..fragment()
    });
    let mut cache = TerminalImageCache::default();
    let mut screen = Terminal::new(TestBackend::new(4, 2)).unwrap();
    screen
        .draw(|frame| {
            frame.buffer_mut()[(1, 1)]
                .set_style(ratatui::style::Style::new().reversed())
                .set_diff_option(ratatui::buffer::CellDiffOption::Skip);
            cache.render(frame, frame.area(), &snapshot, None);
            assert_eq!(
                frame.buffer_mut()[(1, 1)].diff_option,
                ratatui::buffer::CellDiffOption::None
            );
            assert!(
                !frame.buffer_mut()[(1, 1)]
                    .modifier
                    .contains(Modifier::REVERSED)
            );
        })
        .unwrap();
    assert_eq!(
        raster_background(&screen.backend().buffer()[(1, 1)]),
        [Color::Rgb(255, 0, 0); 2]
    );
}

#[test]
fn deeply_negative_images_respect_the_effective_background_of_inverse_cells() {
    let mut snapshot = CommandLineTerminalSnapshot::blank(4, 2);
    snapshot.images.push(TerminalImageFragment {
        z_index: i32::MIN,
        ..fragment()
    });
    snapshot.set_cell(
        1,
        1,
        CommandLineCell {
            style: crate::CommandLineCellStyle {
                foreground: CommandLineColor::Rgb(0, 0, 255),
                inverse: true,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    let mut cache = TerminalImageCache::default();
    let mut screen = Terminal::new(TestBackend::new(4, 2)).unwrap();
    screen
        .draw(|frame| {
            frame.buffer_mut()[(1, 1)]
                .set_fg(Color::Rgb(0, 0, 255))
                .set_style(ratatui::style::Style::new().reversed());
            cache.render(frame, frame.area(), &snapshot, None);
        })
        .unwrap();
    assert_eq!(
        raster_background(&screen.backend().buffer()[(1, 1)]),
        [Color::Rgb(0, 0, 255); 2]
    );
    assert_ne!(screen.backend().buffer()[(2, 1)].bg, Color::Reset);
}
