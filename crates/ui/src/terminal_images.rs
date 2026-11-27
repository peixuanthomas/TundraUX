//! Image preparation stays separate from child protocols and terminal I/O.
use std::collections::{HashMap, HashSet};
use std::fmt::Write;
use std::sync::Arc;

use image::{DynamicImage, Pixel, Rgba, RgbaImage, imageops::FilterType};
use ratatui::Frame;
use ratatui::buffer::Cell;
use ratatui::layout::{Rect, Size};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::Clear;
use ratatui_image::Image;
use ratatui_image::protocol::{Protocol, halfblocks::Halfblocks, sixel::Sixel};

use crate::{
    CommandLineColor, CommandLineTerminalSnapshot, EDITOR_IMAGE_MAX_PIXELS, EditorGraphicsProtocol,
    EditorImagePicker, TerminalImageFragment,
};

mod kitty;

const CACHE_BUDGET: usize = 128 * 1024 * 1024;
const FITTED_PREVIEW_BUDGET: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ImageKey {
    id: u64,
    preview_id: Option<u64>,
    data: usize,
    source: [u32; 4],
    image_size: (u32, u32),
    area_size: Size,
    padding: [u16; 4],
    source_cell_size: [u16; 2],
    crop: [u32; 4],
    size: Size,
    position: Option<(u16, u16)>,
    backend: Option<EditorGraphicsProtocol>,
    cell_size: (u16, u16),
    tmux: bool,
    backgrounds: Vec<[Color; 2]>,
}

struct PreparedImage {
    protocol: Protocol,
    _rgba: Arc<[u8]>,
    kitty: Option<(u32, bool)>,
    kitty_upload: Option<String>,
    bytes: usize,
    transparent_cells: Vec<(u16, u16)>,
}

/// A native placement that can be encoded off the terminal thread.
#[derive(Clone)]
pub struct WorkspaceImageRequest {
    key: ImageKey,
    image: TerminalImageFragment,
    bytes: usize,
}

impl PartialEq for WorkspaceImageRequest {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

#[derive(Default)]
pub struct EncodedWorkspaceImage(Option<PreparedImage>);
type WorkspaceEncoder<'a> = dyn FnMut(&WorkspaceImageRequest) -> Option<EncodedWorkspaceImage> + 'a;

impl WorkspaceImageRequest {
    pub fn encode(&self) -> EncodedWorkspaceImage {
        EncodedWorkspaceImage(prepare_image(&self.image, &self.key, &mut None, self.bytes))
    }
}

/// One page's native placements and bounded cache of fitted preview pixels.
#[derive(Default)]
pub struct TerminalImageCache {
    entries: HashMap<ImageKey, PreparedImage>,
    failed: HashSet<ImageKey>,
    cleanup: Vec<u8>,
    workspace_origin: Option<Rect>,
    workspace_images: Vec<workspace_protocol::Image>,
    fitted_previews: HashMap<(u64, u32, u32), (u64, Arc<[u8]>)>,
    fitted_preview_bytes: usize,
}

impl TerminalImageCache {
    pub fn render(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        terminal: &CommandLineTerminalSnapshot,
        picker: Option<&EditorImagePicker>,
    ) {
        let clipped = area.intersection(frame.area());
        let viewport = Rect::new(
            clipped.x.saturating_sub(area.x),
            clipped.y.saturating_sub(area.y),
            clipped.width,
            clipped.height,
        );
        let mut used = HashSet::new();
        let mut images = terminal.images.iter().collect::<Vec<_>>();
        let raster_layers = images.iter().any(|image| {
            !image.area.intersection(viewport).is_empty()
                && (image.padding != [0; 4]
                    || image.rgba.chunks_exact(4).any(|pixel| pixel[3] != 255))
        }) || images.iter().enumerate().any(|(index, image)| {
            images[index + 1..].iter().any(|other| {
                !image
                    .area
                    .intersection(other.area)
                    .intersection(viewport)
                    .is_empty()
            })
        });
        // Native payloads clear cells or use spanning placeholders, so layers must compose first.
        let picker = picker.filter(|_| !raster_layers);
        images.sort_by_key(|image| image.z_index);
        for image in images {
            let visible = image.area.intersection(viewport);
            let pieces = visible_pieces(image, visible, terminal);
            if !raster_layers && let Some(origin) = self.workspace_origin {
                self.capture_workspace_image(area, origin, image, &pieces, None);
            }
            let mut canvas = None;
            for piece in pieces {
                let Some(crop) = source_for_area(image, piece) else {
                    continue;
                };
                let Some((stored, working)) =
                    preparation_cost(image, piece.as_size(), crop, picker)
                else {
                    continue;
                };
                if image
                    .rgba
                    .len()
                    .saturating_add(stored)
                    .saturating_add(working)
                    > CACHE_BUDGET
                {
                    continue;
                }
                let key_bytes = if picker.is_none() {
                    usize::from(piece.width)
                        * usize::from(piece.height)
                        * std::mem::size_of::<[Color; 2]>()
                } else {
                    0
                };
                if self.cached_bytes().saturating_add(key_bytes) > CACHE_BUDGET {
                    self.retain_used(&used);
                    if self.cached_bytes().saturating_add(key_bytes) > CACHE_BUDGET {
                        continue;
                    }
                }
                let target = offset_rect(piece, area.x, area.y);
                let key = ImageKey::new(image, crop, target, frame, picker);
                self.render_piece(
                    frame,
                    image,
                    &mut canvas,
                    target,
                    key,
                    stored,
                    working,
                    &mut used,
                    None,
                    Color::Reset,
                );
            }
        }
        self.retain_used(&used);
    }

    /// Collects native image placements in pane-local coordinates while retaining raster fallback.
    pub fn capture_workspace(&mut self, area: Rect) {
        self.workspace_origin = Some(area);
        self.workspace_images.clear();
    }

    pub fn take_workspace_images(&mut self) -> Vec<workspace_protocol::Image> {
        self.workspace_origin = None;
        let mut images = std::mem::take(&mut self.workspace_images);
        for image in &mut images {
            let width = u32::from(image.area.width) * u32::from(image.cell_size[0]);
            let height = u32::from(image.area.height) * u32::from(image.cell_size[1]);
            // A fitted preview only needs the pixels the terminal can show. Crops keep the source for panning.
            if image.preview_id.is_none()
                || image.source != [0, 0, image.width, image.height]
                || width > image.width
                || height > image.height
                || u64::from(width) * u64::from(height)
                    > u64::from(image.width) * u64::from(image.height) / 2
            {
                continue;
            }
            let Some((id, rgba)) = self.fitted_preview_pixels(
                image.image_id,
                &image.rgba,
                [image.width, image.height],
                [width, height],
            ) else {
                continue;
            };
            image.image_id = id;
            image.rgba = rgba;
            image.width = width;
            image.height = height;
            image.source = [0, 0, width, height];
        }
        images
    }

    pub(crate) fn fitted_preview_pixels(
        &mut self,
        id: u64,
        rgba: &[u8],
        [source_width, source_height]: [u32; 2],
        [width, height]: [u32; 2],
    ) -> Option<(u64, Arc<[u8]>)> {
        let key = (id, width, height);
        if let Some(fitted) = self.fitted_previews.get(&key) {
            return Some(fitted.clone());
        }
        let bytes = bounded_pixels(u64::from(width), u64::from(height))? as usize * 4;
        if bytes > FITTED_PREVIEW_BUDGET {
            return None;
        }
        let pixels = crate::editor_media::resize_preview_pixels(
            rgba,
            (source_width, source_height),
            [width, height],
        )?;
        if self.fitted_preview_bytes + bytes > FITTED_PREVIEW_BUDGET {
            self.fitted_previews.clear();
            self.fitted_preview_bytes = 0;
        }
        let id = (1 << 63) | u64::from(crate::editor_media::next_kitty_image_id());
        let fitted = (id, pixels.into_raw().into());
        self.fitted_preview_bytes += bytes;
        self.fitted_previews.insert(key, fitted.clone());
        Some(fitted)
    }

    pub(crate) fn capturing_workspace(&self) -> bool {
        self.workspace_origin.is_some()
    }

    /// Registers an editor preview together with its separately drawn cell fallback.
    pub(crate) fn capture_preview(
        &mut self,
        area: Rect,
        frame_area: Rect,
        image: &TerminalImageFragment,
        preview_id: u64,
    ) {
        if let Some(origin) = self.workspace_origin {
            let clipped = area.intersection(frame_area);
            let visible = image.area.intersection(Rect::new(
                clipped.x.saturating_sub(area.x),
                clipped.y.saturating_sub(area.y),
                clipped.width,
                clipped.height,
            ));
            self.capture_workspace_image(area, origin, image, &[visible], Some(preview_id));
        }
    }

    fn capture_workspace_image(
        &mut self,
        terminal_area: Rect,
        content: Rect,
        image: &TerminalImageFragment,
        pieces: &[Rect],
        preview_id: Option<u64>,
    ) {
        fn wire_rect(rect: Rect, content: Rect) -> workspace_protocol::Rect {
            workspace_protocol::Rect {
                x: rect.x - content.x,
                y: rect.y - content.y,
                width: rect.width,
                height: rect.height,
            }
        }

        let area = offset_rect(image.area, terminal_area.x, terminal_area.y);
        if !content.contains((area.x, area.y).into()) {
            return;
        }
        let visible = pieces
            .iter()
            .filter_map(|piece| {
                let clipped =
                    offset_rect(*piece, terminal_area.x, terminal_area.y).intersection(content);
                (!clipped.is_empty()).then(|| wire_rect(clipped, content))
            })
            .collect::<Vec<_>>();
        if visible.is_empty() {
            return;
        }
        self.workspace_images.push(workspace_protocol::Image {
            image_id: image.image_id,
            preview_id,
            width: image.width,
            height: image.height,
            rgba: image.rgba.clone(),
            source: image.source,
            padding: image.padding,
            cell_size: image.cell_size,
            area: wire_rect(area, content),
            visible,
            z_index: image.z_index,
        });
    }

    /// Encodes the worker's visible image fragments with the host's native graphics protocol.
    pub fn render_workspace(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        images: &[workspace_protocol::Image],
        picker: &EditorImagePicker,
    ) {
        self.render_workspace_with(frame, area, images, picker, Color::Reset, None);
    }

    /// The encoder returns `None` while a placement is pending. Editor previews can keep the preceding image.
    pub fn render_workspace_with(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        images: &[workspace_protocol::Image],
        picker: &EditorImagePicker,
        background: Color,
        mut encode: Option<&mut WorkspaceEncoder<'_>>,
    ) {
        let viewport = area.intersection(frame.area());
        let mut used = HashSet::new();
        for asset in images {
            let image = TerminalImageFragment {
                image_id: asset.image_id,
                width: asset.width,
                height: asset.height,
                rgba: asset.rgba.clone(),
                source: asset.source,
                padding: asset.padding,
                cell_size: asset.cell_size,
                area: Rect::new(
                    asset.area.x,
                    asset.area.y,
                    asset.area.width,
                    asset.area.height,
                ),
                z_index: asset.z_index,
            };
            let mut canvas = None;
            for visible in &asset.visible {
                let local = Rect::new(visible.x, visible.y, visible.width, visible.height);
                let piece = offset_rect(local.intersection(image.area), area.x, area.y)
                    .intersection(viewport);
                if piece.is_empty() {
                    continue;
                }
                let local = Rect::new(
                    piece.x - area.x,
                    piece.y - area.y,
                    piece.width,
                    piece.height,
                );
                let Some(crop) = source_for_area(&image, local) else {
                    continue;
                };
                let Some((stored, working)) =
                    preparation_cost(&image, piece.as_size(), crop, Some(picker))
                else {
                    continue;
                };
                let mut key = ImageKey::new(&image, crop, piece, frame, Some(picker));
                key.preview_id = asset.preview_id;
                let pending = self.render_piece(
                    frame,
                    &image,
                    &mut canvas,
                    piece,
                    key,
                    stored,
                    working,
                    &mut used,
                    encode.as_deref_mut(),
                    background,
                );
                if let Some(key) = pending
                    && let Some(preview_id) = key.preview_id
                {
                    clear_preview(frame, piece, background);
                    let previous = self.entries.iter().find_map(|(old, prepared)| {
                        let (x, y) = old.position?;
                        let placement = Rect::new(x, y, old.size.width, old.size.height);
                        (old.preview_id == Some(preview_id)
                            && old.backend == key.backend
                            && old.cell_size == key.cell_size
                            && old.tmux == key.tmux
                            && placement.intersection(piece) == placement)
                            .then_some((old, prepared, placement))
                    });
                    if let Some((old, prepared, placement)) = previous {
                        frame.render_widget(Image::new(&prepared.protocol), placement);
                        used.insert(old.clone());
                    }
                }
            }
        }
        self.retain_used(&used);
        self.failed.retain(|key| used.contains(key));
    }

    #[allow(clippy::too_many_arguments)]
    fn render_piece(
        &mut self,
        frame: &mut Frame<'_>,
        image: &TerminalImageFragment,
        canvas: &mut Option<Option<RgbaImage>>,
        target: Rect,
        key: ImageKey,
        stored: usize,
        working: usize,
        used: &mut HashSet<ImageKey>,
        encode: Option<&mut WorkspaceEncoder<'_>>,
        background: Color,
    ) -> Option<ImageKey> {
        if !self.entries.contains_key(&key) {
            if self.failed.contains(&key) {
                used.insert(key);
                return None;
            }
            let needed = |cache: &Self| {
                let source = if cache
                    .entries
                    .values()
                    .any(|entry| Arc::ptr_eq(&entry._rgba, &image.rgba))
                {
                    0
                } else {
                    image.rgba.len()
                };
                cache
                    .cached_bytes()
                    .saturating_add(source)
                    .saturating_add(stored)
                    .saturating_add(working)
            };
            if needed(self) > CACHE_BUDGET {
                self.retain_used(used);
                if needed(self) > CACHE_BUDGET {
                    return None;
                }
            }
            let prepared = if let Some(encode) = encode {
                let request = WorkspaceImageRequest {
                    key: key.clone(),
                    image: image.clone(),
                    bytes: stored,
                };
                match encode(&request) {
                    Some(EncodedWorkspaceImage(Some(prepared))) => prepared,
                    Some(EncodedWorkspaceImage(None)) => {
                        self.failed.insert(key.clone());
                        used.insert(key);
                        return None;
                    }
                    None => return Some(key),
                }
            } else {
                let Some(prepared) = prepare_image(image, &key, canvas, stored) else {
                    return None;
                };
                prepared
            };
            self.entries.insert(key.clone(), prepared);
        }
        let prepared = self.entries.get_mut(&key).unwrap();
        if key.preview_id.is_some() {
            clear_preview(frame, target, background);
        }
        let raster = key.backend.is_none();
        used.insert(key);
        let transparent = prepared
            .transparent_cells
            .iter()
            .map(|&(x, y)| {
                let position = (target.x + x, target.y + y);
                (position, frame.buffer_mut()[position].clone())
            })
            .collect::<Vec<_>>();
        if raster {
            for y in target.y..target.bottom() {
                for x in target.x..target.right() {
                    frame.buffer_mut()[(x, y)].reset();
                }
            }
        }
        frame.render_widget(Image::new(&prepared.protocol), target);
        if let Some(mut upload) = prepared.kitty_upload.take() {
            let cell = &mut frame.buffer_mut()[(target.x, target.y)];
            upload.push_str(cell.symbol());
            cell.set_symbol(&upload);
        }
        for (position, cell) in transparent {
            frame.buffer_mut()[position] = cell;
        }
        None
    }

    fn cached_bytes(&self) -> usize {
        let mut data = HashSet::new();
        self.entries.values().fold(0_usize, |bytes, entry| {
            bytes.saturating_add(entry.bytes).saturating_add(
                if data.insert(entry._rgba.as_ptr() as usize) {
                    entry._rgba.len()
                } else {
                    0
                },
            )
        })
    }

    fn retain_used(&mut self, used: &HashSet<ImageKey>) {
        self.entries.retain(|key, image| {
            if used.contains(key) {
                true
            } else {
                append_cleanup(&mut self.cleanup, image.kitty);
                false
            }
        });
    }

    pub fn clear(&mut self) {
        self.failed.clear();
        for (_, image) in self.entries.drain() {
            append_cleanup(&mut self.cleanup, image.kitty);
        }
    }

    /// Trusted host-resource deletion commands; no child bytes are included.
    pub fn take_cleanup(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.cleanup)
    }
}

impl ImageKey {
    fn new(
        image: &TerminalImageFragment,
        crop: [u32; 4],
        target: Rect,
        frame: &mut Frame<'_>,
        picker: Option<&EditorImagePicker>,
    ) -> Self {
        Self {
            id: image.image_id,
            preview_id: None,
            data: image.rgba.as_ptr() as usize,
            source: image.source,
            image_size: (image.width, image.height),
            area_size: image.area.as_size(),
            padding: image.padding,
            source_cell_size: image.cell_size,
            crop,
            size: target.as_size(),
            position: picker.map(|_| (target.x, target.y)),
            backend: picker.and_then(EditorImagePicker::protocol),
            cell_size: picker.map_or((1, 2), EditorImagePicker::cell_size),
            tmux: picker.is_some_and(EditorImagePicker::is_tmux),
            backgrounds: if picker.is_none() {
                (target.y..target.bottom())
                    .flat_map(|y| (target.x..target.right()).map(move |x| (x, y)))
                    .map(|position| raster_background(&frame.buffer_mut()[position]))
                    .collect()
            } else {
                Vec::new()
            },
        }
    }
}

fn clear_preview(frame: &mut Frame<'_>, area: Rect, background: Color) {
    frame.render_widget(Clear, area);
    frame
        .buffer_mut()
        .set_style(area, Style::default().bg(background));
}

fn offset_rect(rect: Rect, x: u16, y: u16) -> Rect {
    Rect::new(
        rect.x.saturating_add(x),
        rect.y.saturating_add(y),
        rect.width,
        rect.height,
    )
}

fn preparation_cost(
    image: &TerminalImageFragment,
    size: Size,
    crop: [u32; 4],
    picker: Option<&EditorImagePicker>,
) -> Option<(usize, usize)> {
    let cell_size = picker.map_or((1, 2), EditorImagePicker::cell_size);
    bounded_pixels(u64::from(image.width), u64::from(image.height))?;
    let native = picker
        .and_then(EditorImagePicker::protocol)
        .and_then(|protocol| native_raster(image, crop, cell_size, protocol));
    let (pixels, working) = if let Some(native) = native {
        let [source_width, source_height] = [native.source[2], native.source[3]].map(u64::from);
        let [width, height] = native.output.map(u64::from);
        let pixels = bounded_pixels(width, height)?;
        let encoder_working = pixels.checked_mul(16)?;
        let resize_working = if [source_width, source_height] == [width, height] {
            0
        } else {
            let source = bounded_pixels(source_width, source_height)?;
            // Resizing and native encoding use separate temporary buffers.
            let pass = bounded_pixels(source_width, height)?;
            source
                .checked_mul(4)?
                .checked_add(pass.checked_mul(16)?)?
                .checked_add(pixels.checked_mul(4)?)?
        };
        (pixels, encoder_working.max(resize_working))
    } else {
        canvas_preparation_cost(image, size, cell_size)?
    };
    let cells = u64::from(size.width) * u64::from(size.height);
    let keys = cells.checked_mul((std::mem::size_of::<[Color; 2]>() * 3 + 8) as u64)?;
    let stored = pixels.checked_mul(32)?.checked_add(keys)?.checked_add(
        (std::mem::size_of::<ImageKey>() + std::mem::size_of::<PreparedImage>() + 64) as u64,
    )?;
    Some((stored.try_into().ok()?, working.try_into().ok()?))
}

fn bounded_pixels(width: u64, height: u64) -> Option<u64> {
    let count = width.checked_mul(height)?;
    (count > 0 && count <= EDITOR_IMAGE_MAX_PIXELS).then_some(count)
}

fn canvas_preparation_cost(
    image: &TerminalImageFragment,
    size: Size,
    cell_size: (u16, u16),
) -> Option<(u64, u64)> {
    let source = bounded_pixels(u64::from(image.source[2]), u64::from(image.source[3]))?;
    let canvas_width = u64::from(image.area.width) * u64::from(image.cell_size[0]);
    let canvas_height = u64::from(image.area.height) * u64::from(image.cell_size[1]);
    let canvas = bounded_pixels(canvas_width, canvas_height)?;
    let crop_width = u64::from(size.width) * u64::from(image.cell_size[0]);
    let crop = bounded_pixels(
        crop_width,
        u64::from(size.height) * u64::from(image.cell_size[1]),
    )?;
    let target_height = u64::from(size.height) * u64::from(cell_size.1);
    let pixels = bounded_pixels(
        u64::from(size.width) * u64::from(cell_size.0),
        target_height,
    )?;
    let content_height =
        canvas_height.checked_sub(u64::from(image.padding[1]) + u64::from(image.padding[3]))?;
    // Triangle resizing allocates a floating-point row pass before the final pixels.
    let canvas_pass = bounded_pixels(u64::from(image.source[2]), content_height)?;
    let target_pass = bounded_pixels(crop_width, target_height)?;
    let working = source
        .checked_mul(4)?
        .checked_add(canvas.checked_mul(8)?)?
        .checked_add(crop.checked_mul(4)?)?
        .checked_add(pixels.checked_mul(8)?)?
        .checked_add(canvas_pass.checked_mul(16)?)?
        .checked_add(target_pass.checked_mul(16)?)?;
    Some((pixels, working))
}

struct NativeRaster {
    source: [u32; 4],
    output: [u32; 2],
}

fn native_raster(
    image: &TerminalImageFragment,
    crop: [u32; 4],
    cell_size: (u16, u16),
    protocol: EditorGraphicsProtocol,
) -> Option<NativeRaster> {
    if image.padding != [0; 4]
        || image.cell_size != [cell_size.0, cell_size.1]
        || !source_in_bounds(image, image.source)
    {
        return None;
    }
    let [x, y, width, height] = image.source;
    let [left, top, output_width, output_height] = crop;
    let (x, width) = scale_crop_axis(
        left,
        output_width,
        u32::from(image.area.width) * u32::from(cell_size.0),
        x,
        width,
    )?;
    let (y, height) = scale_crop_axis(
        top,
        output_height,
        u32::from(image.area.height) * u32::from(cell_size.1),
        y,
        height,
    )?;
    let mut output = [output_width, output_height];
    if matches!(
        protocol,
        EditorGraphicsProtocol::Iterm2 | EditorGraphicsProtocol::Kitty
    ) {
        // These protocols size placements in cells, so upscaling before transfer adds no detail.
        output = [output_width.min(width), output_height.min(height)];
    }
    Some(NativeRaster {
        source: [x, y, width, height],
        output,
    })
}

fn scale_crop_axis(
    offset: u32,
    length: u32,
    full_length: u32,
    start: u32,
    source_length: u32,
) -> Option<(u32, u32)> {
    let end = offset.checked_add(length)?;
    if length == 0 || end > full_length {
        return None;
    }
    let scale = |position| u64::from(position) * u64::from(source_length);
    let first = (scale(offset) / u64::from(full_length)) as u32;
    let last = scale(end).div_ceil(u64::from(full_length)) as u32;
    Some((start + first, last - first))
}

fn source_in_bounds(image: &TerminalImageFragment, [x, y, width, height]: [u32; 4]) -> bool {
    width > 0
        && height > 0
        && x.checked_add(width).is_some_and(|end| end <= image.width)
        && y.checked_add(height).is_some_and(|end| end <= image.height)
}

fn raster_background(cell: &Cell) -> [Color; 2] {
    let (foreground, background) = if cell.modifier.contains(Modifier::REVERSED) {
        (cell.bg, cell.fg)
    } else {
        (cell.fg, cell.bg)
    };
    match cell.symbol() {
        "▀" => [foreground, background],
        "▄" => [background, foreground],
        "█" => [foreground; 2],
        _ => [background; 2],
    }
}

fn image_canvas(image: &TerminalImageFragment) -> Option<RgbaImage> {
    let canvas_width = u32::from(image.area.width) * u32::from(image.cell_size[0]);
    let canvas_height = u32::from(image.area.height) * u32::from(image.cell_size[1]);
    if canvas_width == 0
        || canvas_height == 0
        || u64::from(canvas_width) * u64::from(canvas_height) > EDITOR_IMAGE_MAX_PIXELS
    {
        return None;
    }
    let [left, top, right, bottom] = image.padding.map(u32::from);
    let content_width = canvas_width.checked_sub(left.checked_add(right)?)?;
    let content_height = canvas_height.checked_sub(top.checked_add(bottom)?)?;
    if content_width == 0 || content_height == 0 {
        return None;
    }
    let cropped = DynamicImage::ImageRgba8(crop_image_source(image, image.source)?);
    let content = resize_rgba(cropped, content_width, content_height);
    let mut canvas = RgbaImage::new(canvas_width, canvas_height);
    image::imageops::overlay(&mut canvas, &content, i64::from(left), i64::from(top));
    Some(canvas)
}

fn crop_image_source(image: &TerminalImageFragment, source: [u32; 4]) -> Option<RgbaImage> {
    let pixels = bounded_pixels(u64::from(image.width), u64::from(image.height))?;
    if pixels.checked_mul(4)? != image.rgba.len() as u64 || !source_in_bounds(image, source) {
        return None;
    }
    let [x, y, width, height] = source;
    let pixels = if source == [0, 0, image.width, image.height] {
        image.rgba.to_vec()
    } else {
        let mut cropped = Vec::with_capacity(width as usize * height as usize * 4);
        for row in y..y + height {
            let start = (row as usize * image.width as usize + x as usize) * 4;
            cropped.extend_from_slice(&image.rgba[start..start + width as usize * 4]);
        }
        cropped
    };
    RgbaImage::from_raw(width, height, pixels)
}

fn prepare_image(
    image: &TerminalImageFragment,
    key: &ImageKey,
    canvas: &mut Option<Option<RgbaImage>>,
    bytes: usize,
) -> Option<PreparedImage> {
    let native = key
        .backend
        .and_then(|protocol| native_raster(image, key.crop, key.cell_size, protocol));
    let tmux = key.tmux;
    let prepare_kitty = |rgba: &[u8], pixels| {
        let id = crate::editor_media::next_kitty_image_id();
        let (protocol, upload) = kitty::encode(rgba, pixels, key.size, id, tmux)?;
        Some(PreparedImage {
            protocol,
            _rgba: image.rgba.clone(),
            kitty: Some((id, tmux)),
            kitty_upload: Some(upload),
            transparent_cells: Vec::new(),
            bytes,
        })
    };
    if key.backend == Some(EditorGraphicsProtocol::Kitty)
        && native.as_ref().is_some_and(|native| {
            native.source == [0, 0, image.width, image.height]
                && native.output == [image.width, image.height]
                && image.rgba.len() as u64 == u64::from(image.width) * u64::from(image.height) * 4
        })
    {
        return prepare_kitty(&image.rgba, [image.width, image.height]);
    }

    let mut resized = if let Some(native) = native {
        let crop = crop_image_source(image, native.source)?;
        if [crop.width(), crop.height()] == native.output {
            crop
        } else {
            crate::editor_media::resize_preview_pixels(&crop, crop.dimensions(), native.output)?
        }
    } else {
        let target_width = u32::from(key.size.width).checked_mul(u32::from(key.cell_size.0))?;
        let target_height = u32::from(key.size.height).checked_mul(u32::from(key.cell_size.1))?;
        let canvas = canvas.get_or_insert_with(|| image_canvas(image)).as_ref()?;
        let [x, y, width, height] = key.crop;
        let cropped = DynamicImage::ImageRgba8(
            image::imageops::crop_imm(canvas, x, y, width, height).to_image(),
        );
        resize_rgba(cropped, target_width, target_height)
    };
    let mut transparent_cells = Vec::new();
    if key.backend.is_none() {
        let mut transparent =
            vec![true; usize::from(key.size.width) * usize::from(key.size.height)];
        for (x, y, pixel) in resized.enumerate_pixels_mut() {
            let column = x / u32::from(key.cell_size.0);
            let row = y / u32::from(key.cell_size.1);
            let index = row as usize * usize::from(key.size.width) + column as usize;
            transparent[index] &= pixel[3] == 0;
            let half = usize::from(
                y % u32::from(key.cell_size.1) >= u32::from(key.cell_size.1).div_ceil(2),
            );
            let (red, green, blue) =
                crate::theme::color_rgb(key.backgrounds[index][half]).unwrap_or((0, 0, 0));
            let mut background = Rgba([red, green, blue, 255]);
            background.blend(pixel);
            *pixel = background;
        }
        transparent_cells = transparent
            .into_iter()
            .enumerate()
            .filter_map(|(index, transparent)| {
                transparent.then_some((
                    (index % usize::from(key.size.width)) as u16,
                    (index / usize::from(key.size.width)) as u16,
                ))
            })
            .collect();
    }
    let resized = DynamicImage::ImageRgba8(resized);
    let protocol = match key.backend {
        Some(EditorGraphicsProtocol::Kitty) => {
            return prepare_kitty(resized.as_bytes(), [resized.width(), resized.height()]);
        }
        Some(EditorGraphicsProtocol::Sixel) => {
            Protocol::Sixel(Sixel::new(resized, key.size, tmux).ok()?)
        }
        Some(EditorGraphicsProtocol::Iterm2) => {
            Protocol::ITerm2(crate::editor_media::prepare_iterm2(resized, key.size, tmux).ok()?)
        }
        None => Protocol::Halfblocks(Halfblocks::new(resized, key.size).ok()?),
    };
    Some(PreparedImage {
        protocol,
        _rgba: image.rgba.clone(),
        kitty: None,
        kitty_upload: None,
        transparent_cells,
        bytes,
    })
}

fn resize_rgba(image: DynamicImage, width: u32, height: u32) -> RgbaImage {
    if image.width() == width && image.height() == height {
        image.into_rgba8()
    } else {
        image
            .resize_exact(width, height, FilterType::Triangle)
            .into_rgba8()
    }
}

fn source_for_area(image: &TerminalImageFragment, area: Rect) -> Option<[u32; 4]> {
    if area.is_empty() || image.area.is_empty() || image.cell_size.contains(&0) {
        return None;
    }
    Some([
        u32::from(area.x.saturating_sub(image.area.x)) * u32::from(image.cell_size[0]),
        u32::from(area.y.saturating_sub(image.area.y)) * u32::from(image.cell_size[1]),
        u32::from(area.width) * u32::from(image.cell_size[0]),
        u32::from(area.height) * u32::from(image.cell_size[1]),
    ])
}

fn visible_pieces(
    image: &TerminalImageFragment,
    visible: Rect,
    terminal: &CommandLineTerminalSnapshot,
) -> Vec<Rect> {
    if visible.is_empty() {
        return Vec::new();
    }
    if image.z_index >= 0 {
        return vec![visible];
    }
    let mut pieces = Vec::new();
    for y in visible.y..visible.bottom() {
        let mut start = None;
        for x in visible.x..visible.right() {
            let text = terminal.cell(x, y).is_some_and(|cell| {
                cell.cursor
                    || cell.command_status.is_some()
                    || !cell.symbol.trim().is_empty()
                    || image.z_index < -(1 << 30)
                        && if cell.style.inverse {
                            cell.style.foreground != CommandLineColor::Default
                        } else {
                            cell.style.background != CommandLineColor::Default
                        }
            }) || x > 0
                && terminal
                    .cell(x - 1, y)
                    .is_some_and(|cell| ratatui::text::Span::raw(&cell.symbol).width() > 1);
            if !text {
                start.get_or_insert(x);
            } else if let Some(left) = start.take() {
                pieces.push(Rect::new(left, y, x - left, 1));
            }
        }
        if let Some(left) = start {
            pieces.push(Rect::new(left, y, visible.right() - left, 1));
        }
    }
    pieces
}

fn append_cleanup(output: &mut Vec<u8>, kitty: Option<(u32, bool)>) {
    if let Some((id, tmux)) = kitty {
        let mut command = String::new();
        let _ = write!(command, "\x1b_Ga=d,d=I,i={id}\x1b\\");
        if tmux {
            output.extend_from_slice(b"\x1bPtmux;");
            output.extend_from_slice(command.replace('\x1b', "\x1b\x1b").as_bytes());
            output.extend_from_slice(b"\x1b\\");
        } else {
            output.extend_from_slice(command.as_bytes());
        }
    }
}

#[cfg(test)]
#[path = "../tests/unit/terminal_images.rs"]
mod tests;
