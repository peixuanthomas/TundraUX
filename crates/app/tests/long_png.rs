use std::borrow::Cow;
use std::cell::Cell;
use std::io::{self, Cursor, Write};

use app::editor::{EditorState, PreviewImage};
use flate2::{Compression, write::ZlibEncoder};
use image::{
    AnimationDecoder, DynamicImage, ImageError, ImageFormat, Rgba, RgbaImage,
    codecs::png::PngDecoder, metadata::Orientation,
};

const LONG_PNG: &[u8] = include_bytes!("fixtures/long-bands.png");

fn open_png(bytes: &[u8]) -> EditorState {
    EditorState::open_preview_image("reading.png".into(), bytes, || false).unwrap()
}

fn strips(editor: &EditorState) -> Vec<&PreviewImage> {
    (0..)
        .map_while(|index| editor.office_image(&format!("tundra-preview-image/{index}")))
        .collect()
}

fn assert_strip_pixels(editor: &EditorState, expected: &RgbaImage) {
    let mut y = 0;
    for strip in strips(editor) {
        assert_eq!(strip.width, expected.width());
        let stride = strip.width as usize * 4;
        assert_eq!(strip.rgba.len(), stride * strip.height as usize);
        for row in strip.rgba.chunks_exact(stride) {
            let offset = y * stride;
            assert_eq!(row, &expected.as_raw()[offset..offset + stride], "row {y}");
            y += 1;
        }
    }
    assert_eq!(y, expected.height() as usize);
}

fn encode_png_with_exif(
    (width, height): (u32, u32),
    color: png::ColorType,
    pixels: &[u8],
    orientation: u8,
) -> Vec<u8> {
    let mut exif = *b"II*\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\0\0\0\0\0\0\0\0";
    // The orientation is the value of the sole TIFF directory entry.
    exif[18] = orientation;
    let mut info = png::Info::with_size(width, height);
    info.color_type = color;
    info.exif_metadata = Some(Cow::Borrowed(&exif));
    let mut bytes = Vec::new();
    let mut writer = png::Encoder::with_info(&mut bytes, info)
        .unwrap()
        .write_header()
        .unwrap();
    writer.write_image_data(pixels).unwrap();
    writer.finish().unwrap();
    bytes
}

#[test]
fn a_tall_png_keeps_all_rows_in_prepared_read_only_strips() {
    let editor = open_png(LONG_PNG);
    assert!(editor.is_read_only());
    assert!(editor.is_scrollable_picture());
    let strips = strips(&editor);
    assert!(strips.len() > 1);
    assert!(strips.iter().all(|strip| strip.width == 128));
    assert_eq!(strips.iter().map(|strip| strip.height).sum::<u32>(), 18_432);
    assert_eq!(strips[0].sample(0, 0, 1, 1), [230, 20, 30, 255]);
    assert_eq!(
        strips[strips.len() / 2].sample(0, 0, 1, 1),
        [30, 180, 60, 255]
    );
    assert_eq!(
        strips.last().unwrap().sample(0, 0, 1, 1),
        [40, 70, 220, 255]
    );
}

#[test]
fn adam7_strips_match_a_full_decode_across_pass_and_strip_boundaries() {
    let passes = [
        (0, 0, 8, 8),
        (4, 0, 8, 8),
        (0, 4, 4, 8),
        (2, 0, 4, 4),
        (0, 2, 2, 4),
        (1, 0, 2, 2),
        (0, 1, 1, 2),
    ];
    for width in [3, 9] {
        let height = 2051;
        let expected = RgbaImage::from_fn(width, height, |x, y| {
            Rgba([
                x as u8 * 19,
                (y % 251) as u8,
                (y / 16 % 251) as u8,
                ((x + y) % 5 * 63) as u8,
            ])
        });
        let mut scanlines = Vec::new();
        for (x, y, dx, dy) in passes {
            if x >= width {
                continue;
            }
            for row in (y..height).step_by(dy) {
                scanlines.push(0);
                for column in (x..width).step_by(dx) {
                    scanlines.extend_from_slice(&expected.get_pixel(column, row).0);
                }
            }
        }
        let mut compressed = ZlibEncoder::new(Vec::new(), Compression::fast());
        compressed.write_all(&scanlines).unwrap();
        let mut info = png::Info::with_size(width, height);
        info.color_type = png::ColorType::Rgba;
        info.interlaced = true;
        let mut bytes = Vec::new();
        let mut writer = png::Encoder::with_info(&mut bytes, info)
            .unwrap()
            .write_header()
            .unwrap();
        writer
            .write_chunk(png::chunk::IDAT, &compressed.finish().unwrap())
            .unwrap();
        writer.finish().unwrap();

        let full = image::load_from_memory_with_format(&bytes, ImageFormat::Png)
            .unwrap()
            .into_rgba8();
        assert!(
            full == expected,
            "Adam7 fixture differs from its source pixels"
        );
        assert_strip_pixels(&open_png(&bytes), &full);
    }
}

#[test]
fn tall_apng_uses_the_first_animation_frame_over_a_transparent_canvas() {
    let (width, height) = (5, 2051);
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_animated(2, 0).unwrap();
    encoder.set_sep_def_img(true).unwrap();
    let mut writer = encoder.write_header().unwrap();
    let poster = [230, 20, 30, 255];
    writer
        .write_image_data(&poster.repeat((width * height) as usize))
        .unwrap();
    writer.set_frame_dimension(3, 3).unwrap();
    writer.set_frame_position(2, 511).unwrap();
    let green = [30, 180, 60, 137];
    writer.write_image_data(&green.repeat(9)).unwrap();
    writer
        .write_image_data(&[40, 70, 220, 255].repeat(9))
        .unwrap();
    writer.finish().unwrap();

    let still = image::load_from_memory_with_format(&bytes, ImageFormat::Png)
        .unwrap()
        .into_rgba8();
    assert_eq!(still.get_pixel(2, 511).0, poster);
    let first = PngDecoder::new(Cursor::new(&bytes))
        .unwrap()
        .apng()
        .unwrap()
        .into_frames()
        .next()
        .unwrap()
        .unwrap()
        .into_buffer();
    assert_eq!(first.get_pixel(2, 511).0, green);
    assert_eq!(first.get_pixel(4, 512).0, green);
    assert_eq!(first.get_pixel(1, 511).0, [0; 4]);
    assert_strip_pixels(&open_png(&bytes), &first);
}

#[test]
fn long_png_exif_orientations_match_full_image_transforms() {
    for (width, height) in [(5, 2051), (2051, 5)] {
        let source = RgbaImage::from_fn(width, height, |x, y| {
            Rgba([
                (x % 251) as u8,
                (y % 251) as u8,
                ((x + y) / 251) as u8,
                ((x + y) % 253) as u8,
            ])
        });
        for exif in 1..=8 {
            let bytes = encode_png_with_exif(
                source.dimensions(),
                png::ColorType::Rgba,
                source.as_raw(),
                exif,
            );
            let mut expected = DynamicImage::ImageRgba8(source.clone());
            let tall = (width < height) != (exif >= 5);
            if !tall {
                expected = expected.thumbnail(1536, 1536);
            }
            expected.apply_orientation(Orientation::from_exif(exif).unwrap());

            let editor = open_png(&bytes);
            let heights: Vec<_> = strips(&editor).iter().map(|strip| strip.height).collect();
            assert_eq!(
                editor.is_scrollable_picture(),
                tall,
                "EXIF {exif}, {width}×{height}"
            );
            if tall {
                let expected_heights = if [3, 4, 7, 8].contains(&exif) {
                    [3, 512, 512, 512, 512]
                } else {
                    [512, 512, 512, 512, 3]
                };
                assert_eq!(heights, expected_heights, "EXIF {exif}, {width}×{height}");
            } else {
                assert_eq!(
                    heights,
                    [expected.height()],
                    "EXIF {exif}, {width}×{height}"
                );
            }
            assert_strip_pixels(&editor, &expected.into_rgba8());
        }
    }
}

#[test]
fn a_wide_png_with_exif_orientation_is_scaled_and_split_as_a_tall_image() {
    let (width, height) = (3203, 1600);
    let source_pixel = |x: u32, y: u32| {
        [
            (x / 7 % 251) as u8,
            (y / 9 % 251) as u8,
            (x / 257 + y / 193) as u8,
        ]
    };
    let source = image::RgbImage::from_fn(width, height, |x, y| image::Rgb(source_pixel(x, y)));
    let bytes = encode_png_with_exif(source.dimensions(), png::ColorType::Rgb, source.as_raw(), 6);

    let editor = open_png(&bytes);
    let strips = strips(&editor);
    let (out_width, out_height) = (1536, 3074);
    assert_eq!(strips.len(), 7);
    assert_eq!(strips.last().unwrap().height, 2);
    let mut top = 0;
    for strip in strips {
        assert_eq!(strip.width, out_width);
        for y in [0, strip.height / 2, strip.height - 1] {
            for x in [0, 53, 701, out_width - 1] {
                let source_x = ((2 * (top + y) + 1) * width) / (2 * out_height);
                let source_y = ((2 * (out_width - x) - 1) * height) / (2 * out_width);
                let [r, g, b] = source_pixel(source_x, source_y);
                let offset = ((y * out_width + x) * 4) as usize;
                assert_eq!(
                    &strip.rgba[offset..offset + 4],
                    &[r, g, b, 255],
                    "pixel ({x}, {})",
                    top + y
                );
            }
        }
        top += strip.height;
    }
    assert_eq!(top, out_height);
}

#[test]
fn a_one_pixel_wide_png_shrinks_to_the_height_limit_without_losing_the_bottom() {
    let height = 140_005;
    let shade = |row| (row / 1024 % 251) as u8;
    let source: Vec<_> = (0..height).flat_map(|row| [shade(row), 193]).collect();
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, 1, height);
    encoder.set_color(png::ColorType::GrayscaleAlpha);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(&source).unwrap();
    writer.finish().unwrap();

    let editor = open_png(&bytes);
    let strips = strips(&editor);
    assert_eq!(strips.len(), 256);
    assert!(
        strips
            .iter()
            .all(|strip| strip.width == 1 && strip.height == 512)
    );
    assert_eq!(&strips[0].rgba[..4], &[0, 0, 0, 193]);
    let bottom = strips.last().unwrap().rgba.last_chunk::<4>().unwrap();
    let last_shade = shade(height - 1);
    assert_eq!(bottom, &[last_shade, last_shade, last_shade, 193]);
}

#[test]
fn large_pngs_open_on_either_side_of_the_tall_picture_aspect_ratio() {
    for (width, height, scroll) in [(3000_u32, 5700_u32, true), (12_000, 8, false)] {
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Grayscale);
        encoder.set_depth(png::BitDepth::One);
        let mut writer = encoder.write_header().unwrap();
        writer
            .write_image_data(&vec![255; (width.div_ceil(8) * height) as usize])
            .unwrap();
        writer.finish().unwrap();
        let editor = open_png(&bytes);
        assert_eq!(editor.is_scrollable_picture(), scroll);
        let pictures = strips(&editor);
        assert_eq!(pictures[0].width, 1536);
        assert_eq!(pictures.len() > 1, scroll);
        assert_eq!(
            pictures.last().unwrap().rgba.last_chunk::<4>().unwrap(),
            &[255; 4]
        );
    }
}

#[test]
fn cancelling_a_tall_png_during_decode_returns_interrupted() {
    let polls = Cell::new(0);
    let result = EditorState::open_preview_image("reading.png".into(), LONG_PNG, || {
        polls.set(polls.get() + 1);
        polls.get() >= 31
    });
    let Err(error) = result else {
        panic!("cancelled PNG yielded a preview")
    };
    assert!(
        matches!(&error, ImageError::IoError(error) if error.kind() == io::ErrorKind::Interrupted),
        "{error}"
    );
    assert_eq!(polls.get(), 31);
}

#[test]
fn a_tall_png_with_truncated_or_corrupt_data_returns_an_error() {
    let truncated = &LONG_PNG[..LONG_PNG.len() - 13];
    let mut corrupt = LONG_PNG.to_vec();
    *corrupt.last_mut().unwrap() ^= 1;
    for bytes in [truncated, &corrupt] {
        assert!(EditorState::open_preview_image("reading.png".into(), bytes, || false).is_err());
    }
}
