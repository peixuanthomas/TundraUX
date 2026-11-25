use app::editor::PreviewImage;
use ratatui::layout::Size;
use ratatui::style::Color;

use super::document::DisplayLineRole;
use super::*;
use crate::{
    CommandLineTerminalSnapshot, EditorImagePicker, RenderContext, TerminalImageCache,
    TerminalImageFragment,
};

#[derive(Clone, Copy)]
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
        let scale = (f64::from(self.cells.width) / width)
            .min(f64::from(self.cells.height) / height)
            .min(1.0);
        Size::new(
            (width * scale)
                .ceil()
                .clamp(1.0, f64::from(self.cells.width)) as u16,
            (height * scale)
                .ceil()
                .clamp(1.0, f64::from(self.cells.height)) as u16,
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
    let mut native = Vec::new();
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
        let area = Rect::new(
            (canvas.width - size.width) / 2,
            line as u16,
            size.width,
            height,
        );
        for y in 0..height {
            for x in 0..size.width {
                let sample = |half| {
                    let pixel = image.sample(
                        u32::from(x),
                        2 * u32::from(row + y) + half,
                        u32::from(size.width),
                        2 * u32::from(size.height),
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
                let point = (canvas.x + area.x + x, canvas.y + area.y + y);
                if let Some(cell) = frame.buffer_mut().cell_mut(point) {
                    let (top, bottom) = (sample(0), sample(1));
                    cell.reset();
                    cell.set_symbol(if top == bottom { " " } else { "▀" })
                        .set_fg(top)
                        .set_bg(bottom);
                }
            }
        }
        if picker.is_some() || images.capturing_workspace() {
            let source_y = u32::from(row) * image.height / u32::from(size.height);
            let source_end =
                (u32::from(row + height) * image.height).div_ceil(u32::from(size.height));
            // The transport reuses pixels by ID. A theme change gives a transparent picture new pixels.
            let image_id = if image.is_opaque() {
                *id
            } else {
                (1_u64 << 62) | (id << 24) | u64::from(u32::from_be_bytes([0, r, g, b]))
            };
            let fragment = TerminalImageFragment {
                image_id,
                width: image.width,
                height: image.height,
                rgba: image.opaque_rgba(background),
                source: [0, source_y, image.width, source_end - source_y],
                padding: [0; 4],
                cell_size: model.image_cell_size,
                area,
                z_index: 0,
            };
            if picker.is_some() {
                native.push(fragment);
            } else {
                images.capture_opaque_workspace(canvas, frame.area(), &fragment);
            }
        }
        line += usize::from(height);
    }
    if native.is_empty() {
        images.clear();
    } else {
        images.render(
            frame,
            canvas,
            &CommandLineTerminalSnapshot {
                columns: canvas.width,
                rows: canvas.height,
                cells: Vec::new(),
                images: native,
                cursor_style: Default::default(),
                scrollback_rows: 0,
                scrollback_offset: 0,
            },
            picker,
        );
    }
}
