use std::io::Cursor;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use image::codecs::png::PngDecoder;
use image::error::{LimitError, LimitErrorKind, ParameterError, ParameterErrorKind};
use image::metadata::Orientation;
use image::{
    AnimationDecoder, DynamicImage, ImageDecoder, ImageError, ImageFormat, ImageReader, Limits,
};

use super::{DocumentKind, EditorDocument, EditorState};

mod long_png;

pub(super) const MAX_ENCODED_BYTES: usize = 32 * 1024 * 1024;
const MAX_DECODE_SIDE: u32 = 8192;
const MAX_DECODE_PIXELS: u64 = 16 * 1024 * 1024;
const MAX_PREVIEW_SIDE: u32 = 1536;
const SAMPLE_SIDE: u32 = 256;
const SAMPLE_PIXELS: usize = (SAMPLE_SIDE as usize).pow(2);
const MAX_STORED_BYTES: usize = (2 * (MAX_PREVIEW_SIDE as usize).pow(2) + SAMPLE_PIXELS) * 4;
const PREVIEW_IMAGE_PREFIX: &str = "tundra-preview-image/";

/// Prepared RGBA pixels with orientation applied and a thumbnail for terminal sampling.
#[derive(Debug, Clone)]
pub struct PreviewImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<[u8]>,
    thumbnail: Option<Thumbnail>,
    opaque_cache: Option<Arc<Mutex<Option<OpaquePixels>>>>,
    scroll_vertically: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Thumbnail {
    width: u32,
    height: u32,
    rgba: Arc<[u8]>,
}

#[derive(Debug)]
struct OpaquePixels {
    background: [u8; 3],
    rgba: Arc<[u8]>,
}

impl PartialEq for PreviewImage {
    fn eq(&self, other: &Self) -> bool {
        self.width == other.width
            && self.height == other.height
            && self.rgba == other.rgba
            && self.thumbnail == other.thumbnail
            && self.scroll_vertically == other.scroll_vertically
    }
}

impl Eq for PreviewImage {}

impl PreviewImage {
    pub fn decode(bytes: &[u8]) -> Result<Self, ImageError> {
        Self::decode_with_budget(bytes, MAX_STORED_BYTES)
    }

    pub(super) fn decode_with_budget(bytes: &[u8], budget: usize) -> Result<Self, ImageError> {
        if bytes.len() > MAX_ENCODED_BYTES || budget < 4 {
            return Err(ImageError::Limits(LimitError::from_kind(
                LimitErrorKind::InsufficientMemory,
            )));
        }
        let mut limits = Limits::default();
        limits.max_image_width = Some(MAX_DECODE_SIDE);
        limits.max_image_height = Some(MAX_DECODE_SIDE);
        limits.max_alloc = Some(128 * 1024 * 1024);

        let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
        let (mut image, orientation) = if reader.format() == Some(ImageFormat::Png) {
            let mut decoder = PngDecoder::with_limits(reader.into_inner(), limits.clone())?;
            if decoder.is_apng()? {
                limit_decoder(&mut decoder, limits)?;
                let orientation = decoder.orientation()?;
                let first = decoder
                    .apng()?
                    .into_frames()
                    .next()
                    .transpose()?
                    .ok_or_else(|| {
                        ImageError::Parameter(ParameterError::from_kind(
                            ParameterErrorKind::NoMoreData,
                        ))
                    })?;
                (DynamicImage::ImageRgba8(first.into_buffer()), orientation)
            } else {
                decode_still(decoder, limits)?
            }
        } else {
            reader.limits(limits.clone());
            decode_still(reader.into_decoder()?, limits)?
        };

        let maybe_transparent = image.as_rgba8().map_or_else(
            || image.color().has_alpha(),
            |pixels| pixels.pixels().any(|pixel| pixel[3] != 255),
        );
        let side = Self::budgeted_side(budget, maybe_transparent);
        if side == 0 {
            return Err(ImageError::Limits(LimitError::from_kind(
                LimitErrorKind::InsufficientMemory,
            )));
        }
        if image.width().max(image.height()) > side {
            image = image.thumbnail(side, side);
        }
        image.apply_orientation(orientation);
        Ok(Self::from_pixels(image, maybe_transparent, false))
    }

    pub(super) fn budgeted_side(budget: usize, maybe_transparent: bool) -> u32 {
        let bytes_per_pixel = if maybe_transparent { 8 } else { 4 };
        // This budget includes the native composite and the smaller terminal thumbnail.
        let side = if budget > SAMPLE_PIXELS * bytes_per_pixel {
            ((budget - SAMPLE_PIXELS * 4) / bytes_per_pixel)
                .isqrt()
                .max(SAMPLE_SIDE as usize)
        } else {
            (budget / bytes_per_pixel).isqrt()
        };
        side.min(MAX_PREVIEW_SIDE as usize) as u32
    }

    pub(super) fn from_pixels(
        image: DynamicImage,
        maybe_transparent: bool,
        scroll_vertically: bool,
    ) -> Self {
        let thumbnail = (image.width().max(image.height()) > SAMPLE_SIDE).then(|| {
            let pixels = image.thumbnail(SAMPLE_SIDE, SAMPLE_SIDE).into_rgba8();
            Thumbnail {
                width: pixels.width(),
                height: pixels.height(),
                rgba: pixels.into_raw().into(),
            }
        });
        let pixels = image.into_rgba8();
        let opaque_cache = (maybe_transparent && pixels.pixels().any(|pixel| pixel[3] != 255))
            .then(|| Arc::new(Mutex::new(None)));
        Self {
            width: pixels.width(),
            height: pixels.height(),
            rgba: pixels.into_raw().into(),
            thumbnail,
            opaque_cache,
            scroll_vertically,
        }
    }

    /// Long standalone pictures keep their reading width across consecutive strips.
    pub fn scrolls_vertically(&self) -> bool {
        self.scroll_vertically
    }

    /// Whether every stored output pixel is fully opaque.
    pub fn is_opaque(&self) -> bool {
        self.opaque_cache.is_none()
    }

    /// Returns pixels composited onto the terminal background for native graphics protocols.
    pub fn opaque_rgba(&self, background: [u8; 3]) -> Arc<[u8]> {
        let Some(cache) = &self.opaque_cache else {
            return self.rgba.clone();
        };
        let mut cache = cache.lock().expect("preview composite cache");
        if let Some(pixels) = cache
            .as_ref()
            .filter(|pixels| pixels.background == background)
        {
            return pixels.rgba.clone();
        }

        cache.take();
        let mut pixels = Vec::with_capacity(self.rgba.len());
        for pixel in self.rgba.chunks_exact(4) {
            let alpha = u16::from(pixel[3]);
            let blend = |channel: usize| {
                let foreground = u16::from(pixel[channel]) * alpha;
                let back = u16::from(background[channel]) * (255 - alpha);
                ((foreground + back + 127) / 255) as u8
            };
            pixels.extend([blend(0), blend(1), blend(2), 255]);
        }
        let rgba: Arc<[u8]> = pixels.into();
        *cache = Some(OpaquePixels {
            background,
            rgba: rgba.clone(),
        });
        rgba
    }

    /// Samples the prepared thumbnail at the center of an output grid cell.
    pub fn sample(&self, x: u32, y: u32, columns: u32, half_rows: u32) -> [u8; 4] {
        let (width, height, rgba) = self
            .thumbnail
            .as_ref()
            .map_or((self.width, self.height, &self.rgba), |thumbnail| {
                (thumbnail.width, thumbnail.height, &thumbnail.rgba)
            });
        sample_rgba(rgba, [width, height], [x, y], [columns, half_rows])
    }

    /// Samples the original preview pixels for a magnified region.
    pub fn sample_full(&self, x: u32, y: u32, columns: u32, half_rows: u32) -> [u8; 4] {
        sample_rgba(
            &self.rgba,
            [self.width, self.height],
            [x, y],
            [columns, half_rows],
        )
    }

    /// Bytes for the image and its thumbnail, including room for a cached opaque composite.
    pub fn byte_len(&self) -> usize {
        self.rgba.len()
            + self.thumbnail.as_ref().map_or(0, |image| image.rgba.len())
            + if self.is_opaque() { 0 } else { self.rgba.len() }
    }
}

fn sample_rgba(
    rgba: &[u8],
    [width, height]: [u32; 2],
    [x, y]: [u32; 2],
    [columns, half_rows]: [u32; 2],
) -> [u8; 4] {
    if columns == 0 || half_rows == 0 {
        return [0; 4];
    }
    let x = sample_axis(x, columns, width);
    let y = sample_axis(y, half_rows, height);
    let start = (y * width as usize + x) * 4;
    rgba[start..start + 4].try_into().expect("RGBA pixel")
}

fn limit_decoder(decoder: &mut impl ImageDecoder, mut limits: Limits) -> Result<(), ImageError> {
    let (width, height) = decoder.dimensions();
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > MAX_DECODE_PIXELS {
        return Err(ImageError::Limits(LimitError::from_kind(
            LimitErrorKind::DimensionError,
        )));
    }
    limits.reserve(decoder.total_bytes())?;
    decoder.set_limits(limits)
}

fn decode_still(
    mut decoder: impl ImageDecoder,
    limits: Limits,
) -> Result<(DynamicImage, Orientation), ImageError> {
    limit_decoder(&mut decoder, limits)?;
    let orientation = decoder.orientation()?;
    Ok((DynamicImage::from_decoder(decoder)?, orientation))
}

fn sample_axis(position: u32, cells: u32, pixels: u32) -> usize {
    let pixels = u64::from(pixels);
    let position = u64::from(position.min(cells - 1));
    ((position * pixels + pixels / 2) / u64::from(cells)) as usize
}

pub(super) fn preview_image_url(index: usize) -> String {
    format!("{PREVIEW_IMAGE_PREFIX}{index}")
}

impl EditorState {
    pub fn open_preview_image(
        path: PathBuf,
        bytes: &[u8],
        is_cancelled: impl Fn() -> bool,
    ) -> Result<Self, ImageError> {
        if let Some(strips) = long_png::decode(bytes, is_cancelled)? {
            return Ok(Self::from_preview_images(path, strips));
        }
        Ok(Self::from_preview_image(path, PreviewImage::decode(bytes)?))
    }

    pub fn from_preview_image(path: PathBuf, mut image: PreviewImage) -> Self {
        image.scroll_vertically = u64::from(image.height) >= 2 * u64::from(image.width);
        Self::from_preview_images(path, vec![image])
    }

    fn from_preview_images(path: PathBuf, images: Vec<PreviewImage>) -> Self {
        let markdown = (0..images.len())
            .map(|index| format!("![]({})", preview_image_url(index)))
            .collect::<Vec<_>>()
            .join("\n\n");
        let document = EditorDocument::from_text(Some(path), DocumentKind::Markdown, markdown);
        let mut state = Self::from_read_only_document(document);
        state.preview_images = images;
        state
    }

    pub fn is_scrollable_picture(&self) -> bool {
        self.preview_images
            .first()
            .is_some_and(PreviewImage::scrolls_vertically)
    }

    pub fn office_image(&self, url: &str) -> Option<&PreviewImage> {
        PreviewImage::embedded_from_url(self.embedded_preview_images(), url)
    }

    pub fn embedded_preview_images(&self) -> &[PreviewImage] {
        &self.preview_images
    }
}

impl PreviewImage {
    pub fn embedded_from_url<'a>(images: &'a [Self], url: &str) -> Option<&'a Self> {
        let index: usize = url.strip_prefix(PREVIEW_IMAGE_PREFIX)?.parse().ok()?;
        images.get(index)
    }
}
