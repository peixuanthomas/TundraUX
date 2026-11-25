use std::io::{self, Cursor};

use image::{
    DynamicImage, ImageError, ImageFormat, RgbaImage, error::DecodingError, metadata::Orientation,
};

use super::{
    MAX_DECODE_PIXELS, MAX_DECODE_SIDE, MAX_ENCODED_BYTES, MAX_PREVIEW_SIDE, PreviewImage,
    sample_axis,
};

const STRIP_HEIGHT: u32 = 512;
const MAX_OUTPUT_HEIGHT: u32 = 256 * STRIP_HEIGHT;
const MAX_OUTPUT_PIXELS: u64 = 32 * 1024 * 1024;
const MAX_SOURCE_PIXELS: u64 = 1024 * 1024 * 1024;

// Adam7 row coordinates are relative to the current animation frame.
const ADAM7: &[(u32, u32, u32, u32)] = &[
    (0, 0, 8, 8),
    (4, 0, 8, 8),
    (0, 4, 4, 8),
    (2, 0, 4, 4),
    (0, 2, 2, 4),
    (1, 0, 2, 2),
    (0, 1, 1, 2),
];

pub(super) fn decode(
    bytes: &[u8],
    is_cancelled: impl Fn() -> bool,
) -> Result<Option<Vec<PreviewImage>>, ImageError> {
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Ok(None);
    }
    let mut decoder = png::Decoder::new(Cursor::new(bytes));
    let info = decoder.read_header_info().map_err(png_error)?;
    let (width, height) = (info.width, info.height);
    let source_pixels = u64::from(width) * u64::from(height);
    let oversized = width.max(height) > MAX_DECODE_SIDE
        || source_pixels > MAX_DECODE_PIXELS
        || bytes.len() > MAX_ENCODED_BYTES;
    let elongated = width.max(height) > MAX_PREVIEW_SIDE
        && u64::from(width.max(height)) >= 2 * u64::from(width.min(height));
    if !oversized && !elongated {
        return Ok(None);
    }
    if source_pixels > MAX_SOURCE_PIXELS {
        return Err(png_error(png::DecodingError::LimitsExceeded));
    }
    let check_cancelled = || {
        if is_cancelled() {
            Err(io::Error::from(io::ErrorKind::Interrupted))
        } else {
            Ok(())
        }
    };
    check_cancelled()?;
    decoder.set_ignore_text_chunk(true);
    decoder.set_ignore_iccp_chunk(true);
    decoder.set_transformations(
        png::Transformations::normalize_to_color8() | png::Transformations::ALPHA,
    );
    let mut reader = decoder.read_info().map_err(png_error)?;
    let info = reader.info();
    let orientation = info
        .exif_metadata
        .as_deref()
        .and_then(Orientation::from_exif_chunk)
        .unwrap_or(Orientation::NoTransforms);
    let sideways = orientation.to_exif() >= 5;
    let (display_width, display_height) = if sideways {
        (height, width)
    } else {
        (width, height)
    };
    let scroll_vertically = display_height > display_width;
    if !scroll_vertically && !oversized {
        return Ok(None);
    }
    if info.animation_control.is_some() && info.frame_control.is_none() {
        reader.next_frame_info().map_err(png_error)?;
    }
    check_cancelled()?;
    let height_limit = if scroll_vertically {
        MAX_OUTPUT_HEIGHT
    } else {
        MAX_PREVIEW_SIDE
    };
    let scale = 1.0_f64
        .min(f64::from(MAX_PREVIEW_SIDE) / f64::from(display_width))
        .min(f64::from(height_limit) / f64::from(display_height))
        .min((MAX_OUTPUT_PIXELS as f64 / source_pixels as f64).sqrt());
    let scaled = |dimension| (f64::from(dimension) * scale).floor().max(1.0) as u32;
    let (out_width, out_height) = (scaled(width), scaled(height));
    // Cutting along the display's vertical axis also keeps rotation local to one strip.
    let length = scaled(display_height);
    let strip_height = if scroll_vertically {
        STRIP_HEIGHT
    } else {
        length
    };
    let mut strips: Vec<_> = (0..length)
        .step_by(strip_height as usize)
        .map(|start| {
            let extent = (length - start).min(strip_height);
            if sideways {
                RgbaImage::new(extent, out_height)
            } else {
                RgbaImage::new(out_width, extent)
            }
        })
        .collect();
    read_first_frame(
        &mut reader,
        &mut strips,
        [out_width, out_height],
        sideways,
        &check_cancelled,
    )?;
    reader.finish().map_err(png_error)?;
    if matches!(
        orientation,
        Orientation::FlipVertical
            | Orientation::Rotate180
            | Orientation::Rotate270
            | Orientation::Rotate270FlipH
    ) {
        strips.reverse();
    }
    let mut previews = Vec::with_capacity(strips.len());
    for strip in strips {
        check_cancelled()?;
        let mut image = DynamicImage::ImageRgba8(strip);
        image.apply_orientation(orientation);
        previews.push(PreviewImage::from_pixels(image, true, scroll_vertically));
    }
    Ok(Some(previews))
}

fn read_first_frame(
    reader: &mut png::Reader<Cursor<&[u8]>>,
    strips: &mut [RgbaImage],
    [out_width, out_height]: [u32; 2],
    sideways: bool,
    check_cancelled: &impl Fn() -> io::Result<()>,
) -> Result<(), ImageError> {
    let info = reader.info();
    let (width, height) = (info.width, info.height);
    let (frame_width, frame_height, left, top) =
        info.frame_control.map_or((width, height, 0, 0), |frame| {
            (frame.width, frame.height, frame.x_offset, frame.y_offset)
        });
    let channels = reader.output_color_type().0.samples();
    let strip_extent = if sideways {
        strips[0].width()
    } else {
        strips[0].height()
    };
    let passes = if info.interlaced {
        ADAM7
    } else {
        &[(0, 0, 1, 1)][..]
    };
    for &(x0, y0, dx, dy) in passes {
        if x0 >= frame_width {
            continue;
        }
        let mut columns = vec![Vec::new(); if sideways { strips.len() } else { 1 }];
        for x in 0..out_width {
            let sampled = sample_axis(x, out_width, width) as u32;
            let Some(local) = sampled.checked_sub(left + x0) else {
                continue;
            };
            if sampled - left < frame_width && local.is_multiple_of(dx) {
                let (group, dest) = if sideways {
                    (x / strip_extent, x % strip_extent)
                } else {
                    (0, x)
                };
                columns[group as usize].push((dest as usize * 4, (local / dx) as usize * channels));
            }
        }
        let full_row = channels == 4 && dx == 1 && frame_width == width && out_width == width;
        let mut next_y = 0;
        for y in (y0..frame_height).step_by(dy as usize) {
            check_cancelled()?;
            let row = reader
                .next_row()
                .map_err(png_error)?
                .ok_or_else(|| io::Error::from(io::ErrorKind::UnexpectedEof))?;
            let source_y = (top + y) as usize;
            while next_y < out_height && sample_axis(next_y, out_height, height) < source_y {
                next_y += 1;
            }
            if next_y == out_height || sample_axis(next_y, out_height, height) != source_y {
                continue;
            }
            let data = row.data();
            for (group, columns) in columns.iter().enumerate() {
                let (strip, y) = if sideways {
                    (group, next_y)
                } else {
                    ((next_y / strip_extent) as usize, next_y % strip_extent)
                };
                let strip = &mut strips[strip];
                let stride = strip.width() as usize * 4;
                let offset = y as usize * stride;
                let target = &mut strip.as_mut()[offset..offset + stride];
                if full_row {
                    let start = group * strip_extent as usize * 4;
                    target.copy_from_slice(&data[start..start + stride]);
                } else {
                    for &(dest, src) in columns {
                        let pixel = &mut target[dest..dest + 4];
                        if channels == 2 {
                            pixel.copy_from_slice(&[
                                data[src],
                                data[src],
                                data[src],
                                data[src + 1],
                            ]);
                        } else {
                            pixel.copy_from_slice(&data[src..src + 4]);
                        }
                    }
                }
            }
            next_y += 1;
        }
    }
    reader.next_row().map_err(png_error)?;
    Ok(())
}

fn png_error(error: png::DecodingError) -> ImageError {
    ImageError::Decoding(DecodingError::new(ImageFormat::Png.into(), error))
}
