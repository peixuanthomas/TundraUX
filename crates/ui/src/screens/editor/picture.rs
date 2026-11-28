use app::editor::PreviewImage;
use ratatui::layout::Size;
use ratatui::style::Color;

use super::document::{DisplayLineRole, DisplayText, display_run_width};
use super::preview_zoom::preview_pixel_crop;
use super::render::preview_run_is_selected;
use super::*;
use crate::{EditorImagePicker, RenderContext, TerminalImageCache, TerminalImageFragment};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PictureBounds {
    cells: Size,
    cell_size: [u16; 2],
}

pub(super) fn bounds(width: usize, height: usize, cell_size: [u16; 2]) -> PictureBounds {
    PictureBounds {
        cells: Size::new(layout::to_u16(width).max(1), layout::to_u16(height).max(1)),
        cell_size: cell_size.map(|side| side.max(1)),
    }
}

impl PictureBounds {
    pub(super) fn fit(self, image: &PreviewImage) -> Size {
        let [cw, ch] = self.cell_size.map(f64::from);
        let width = f64::from(image.width) / cw;
        let height = f64::from(image.height) / ch;
        let max_height = if image.scrolls_vertically() {
            u16::MAX
        } else {
            self.cells.height
        };
        let scale = (f64::from(self.cells.width) / width)
            .min(f64::from(max_height) / height)
            .min(1.0);
        Size::new(
            (width * scale)
                .ceil()
                .clamp(1.0, f64::from(self.cells.width)) as u16,
            (height * scale).ceil().clamp(1.0, f64::from(max_height)) as u16,
        )
    }
}

pub(super) fn render(
    frame: &mut Frame<'_>,
    layout: &EditorLayout,
    model: &EditorViewModel,
    context: &RenderContext,
    images: &mut TerminalImageCache,
    picker: Option<&EditorImagePicker>,
) {
    let canvas = layout.canvas;
    if model.mode != EditorMode::Rich || canvas.is_empty() {
        images.clear();
        return;
    }
    let (r, g, b) = crate::theme::color_rgb(context.theme.editor_canvas).unwrap_or((0, 0, 0));
    let background = [r, g, b];
    let zoomed = layout.picture_window.is_some();
    let slices = if let Some(window) = layout.picture_window {
        zoomed_slices(model, layout.canvas_panel, window)
    } else {
        let mut slices = fitted_slices(layout, model);
        slices.extend(preview_slices(layout, model));
        slices
    };
    if !zoomed && !slices.is_empty() {
        images.scroll = Some(workspace_protocol::ImageScroll {
            top: canvas.y,
            height: canvas.height,
            first_line: layout.visible_start,
        });
    }
    if picker.is_some() {
        images.capture_workspace(canvas);
    }
    for slice in slices {
        if slice.rasterize {
            slice.draw(frame, canvas, context, background, zoomed);
        }
        if images.capturing_workspace() {
            let (fragment, preview_id) = slice.fragment(
                background,
                model.image_cell_size,
                (!zoomed).then_some(&mut *images),
            );
            let rows = (!zoomed).then(|| workspace_protocol::ImageRows {
                first: slice.offset[1] as u16,
                total: slice.size[1] as u16,
            });
            images.capture_preview(canvas, frame.area(), &fragment, preview_id, rows);
        }
    }
    if let Some(picker) = picker {
        let native = images.take_workspace_images();
        images.render_workspace_with(
            frame,
            canvas,
            &native,
            picker,
            context.theme.editor_canvas,
            None,
        );
    } else {
        images.clear();
    }
}

struct PictureSlice<'a> {
    id: u64,
    image: &'a PreviewImage,
    area: Rect,
    offset: [u32; 2],
    size: [u32; 2],
    rasterize: bool,
}

fn fitted_slices<'a>(layout: &EditorLayout, model: &'a EditorViewModel) -> Vec<PictureSlice<'a>> {
    let canvas = layout.canvas;
    let mut slices = Vec::new();
    let mut line = 0;
    while let Some(current) = layout.prepared_lines.get(line) {
        let DisplayLineRole::Picture { row, size } = current.role else {
            line += 1;
            continue;
        };
        let Some(EditorRenderBlock::Picture { id, image }) = current
            .block_index
            .and_then(|i| model.render_blocks().get(i))
        else {
            line += 1;
            continue;
        };
        let height = (size.height - row).min(canvas.height - line as u16);
        slices.push(PictureSlice {
            id: *id,
            image,
            area: Rect::new(
                (canvas.width - size.width) / 2,
                line as u16,
                size.width,
                height,
            ),
            offset: [0, u32::from(row)],
            size: [u32::from(size.width), u32::from(size.height)],
            rasterize: true,
        });
        line += usize::from(height);
    }
    slices
}

fn preview_slices<'a>(layout: &'a EditorLayout, model: &EditorViewModel) -> Vec<PictureSlice<'a>> {
    let mut slices = Vec::<PictureSlice<'_>>::new();
    let mut placements = BTreeMap::<_, usize>::new();
    for (row, line) in layout.prepared_lines.iter().enumerate() {
        if !line
            .runs
            .iter()
            .any(|run| matches!(run.text, DisplayText::Graphic(_)))
        {
            continue;
        }
        let scroll = if line.no_wrap {
            layout.horizontal_scroll
        } else {
            0
        };
        let end = scroll.saturating_add(usize::from(layout.canvas.width));
        let mut column = line.column_start;
        for run in &line.runs {
            if column >= end {
                break;
            }
            let width = display_run_width(run);
            let start = column;
            column = column.saturating_add(width);
            let DisplayText::Graphic(graphic) = &run.text else {
                continue;
            };
            let left = start.max(scroll);
            let right = column.min(end);
            let document_line = layout.prepared_start.saturating_add(row);
            if left >= right
                || preview_run_is_selected(run, layout, model, document_line, start, width)
            {
                continue;
            }
            let id = graphic.graphic.id;
            let y = to_u16(row);
            // Coalesce the visible rows of one placement, including a crop that starts mid-image.
            let key = (id, start, i32::from(y) - i32::from(graphic.row));
            if let Some(slice) = placements
                .get(&key)
                .and_then(|&index| slices.get_mut(index))
                && slice.area.bottom() == y
            {
                slice.area.height += 1;
            } else {
                placements.insert(key, slices.len());
                slices.push(PictureSlice {
                    id,
                    image: &graphic.graphic.image,
                    area: Rect::new(to_u16(left - scroll), y, to_u16(right - left), 1),
                    offset: [(left - start) as u32, u32::from(graphic.row)],
                    size: [
                        u32::from(graphic.size.width),
                        u32::from(graphic.size.height),
                    ],
                    rasterize: graphic.picture,
                });
            }
        }
    }
    slices
}

fn zoomed_pictures(
    model: &EditorViewModel,
    area: Rect,
    zoom: EditorPreviewZoom,
) -> impl Iterator<Item = (u64, &PreviewImage, [u32; 2])> {
    let bounds = bounds(
        usize::from(area.width),
        usize::from(area.height),
        model.image_cell_size,
    );
    model.render_blocks().iter().filter_map(move |block| {
        let EditorRenderBlock::Picture { id, image } = block else {
            return None;
        };
        let size = bounds.fit(image);
        Some((
            *id,
            image,
            [zoom.scale(size.width), zoom.scale(size.height)],
        ))
    })
}

pub(super) fn zoomed_size(
    model: &EditorViewModel,
    area: Rect,
    zoom: EditorPreviewZoom,
) -> [u32; 2] {
    zoomed_pictures(model, area, zoom).fold([0, 0], |[width, height], (_, _, size)| {
        [width.max(size[0]), height + size[1]]
    })
}

fn zoomed_slices(
    model: &EditorViewModel,
    area: Rect,
    window: EditorPreviewWindow,
) -> Vec<PictureSlice<'_>> {
    let [left, top] = window.offset;
    let bottom = top + u32::from(window.area.height);
    let mut row = 0;
    let mut slices = Vec::new();
    for (id, image, size) in zoomed_pictures(model, area, model.picture_zoom.unwrap()) {
        let first = row.max(top);
        let end = (row + size[1]).min(bottom);
        if first < end {
            slices.push(PictureSlice {
                id,
                image,
                area: Rect::new(
                    window.area.x,
                    window.area.y + (first - top) as u16,
                    window.area.width,
                    (end - first) as u16,
                ),
                offset: [left, first - row],
                size,
                rasterize: true,
            });
        }
        row += size[1];
        if row >= bottom {
            break;
        }
    }
    slices
}

impl PictureSlice<'_> {
    fn draw(
        &self,
        frame: &mut Frame<'_>,
        canvas: Rect,
        context: &RenderContext,
        background: [u8; 3],
        zoomed: bool,
    ) {
        let [columns, rows] = self.size;
        let sample_pixel = if zoomed {
            PreviewImage::sample_full
        } else {
            PreviewImage::sample
        };
        for y in 0..self.area.height {
            for x in 0..self.area.width {
                let sample = |half| {
                    let pixel = sample_pixel(
                        self.image,
                        self.offset[0] + u32::from(x),
                        2 * (self.offset[1] + u32::from(y)) + half,
                        columns,
                        2 * rows,
                    );
                    let blend = |i: usize| {
                        let alpha = u16::from(pixel[3]);
                        ((u16::from(pixel[i]) * alpha
                            + u16::from(background[i]) * (255 - alpha)
                            + 127)
                            / 255) as u8
                    };
                    let color = Color::Rgb(blend(0), blend(1), blend(2));
                    if context.capabilities.color == crate::ColorCapability::Ansi {
                        context.capabilities.terminal_colors.nearest_ansi(color)
                    } else {
                        color
                    }
                };
                let point = (canvas.x + self.area.x + x, canvas.y + self.area.y + y);
                if let Some(cell) = frame.buffer_mut().cell_mut(point) {
                    let (top, bottom) = (sample(0), sample(1));
                    cell.reset();
                    cell.set_symbol(if top == bottom { " " } else { "▀" })
                        .set_fg(top)
                        .set_bg(bottom);
                }
            }
        }
    }

    fn fragment(
        &self,
        background: [u8; 3],
        cell_size: [u16; 2],
        fit: Option<&mut TerminalImageCache>,
    ) -> (TerminalImageFragment, u64) {
        let image = self.image;
        let [r, g, b] = background;
        // The transport reuses pixels by ID. A theme change gives a transparent picture new pixels.
        let preview_id = if image.is_opaque() {
            self.id
        } else {
            (1_u64 << 62) | (self.id << 24) | u64::from(u32::from_be_bytes([0, r, g, b]))
        };
        let mut image_id = preview_id;
        let mut rgba = image.opaque_rgba(background);
        let mut pixels = [image.width, image.height];
        if let Some(cache) = fit {
            // Fit the whole placement before deriving its crop so scrolling can reuse the raster.
            let target = std::array::from_fn(|axis| {
                self.size[axis]
                    .saturating_mul(u32::from(cell_size[axis]))
                    .min(pixels[axis])
            });
            if target != pixels
                && let Some((id, fitted)) =
                    cache.fitted_preview_pixels(preview_id, &rgba, pixels, target)
            {
                image_id = id;
                rgba = fitted;
                pixels = target;
            }
        }
        (
            TerminalImageFragment {
                image_id,
                width: pixels[0],
                height: pixels[1],
                rgba,
                source: preview_pixel_crop(self.offset, self.area, self.size, pixels),
                padding: [0; 4],
                cell_size,
                area: self.area,
                z_index: 0,
            },
            preview_id,
        )
    }
}
