use std::sync::Arc;

use app::editor::{PdfPageImage, PdfPageSize};
use image::{ImageBuffer, Rgba};
use ratatui::layout::HorizontalAlignment;
use ratatui::style::Color;

use super::preview_zoom::preview_pixel_crop;
use super::*;
use crate::components::{Button, ButtonRegion, FormDialog, Surface, terminal_width};
use crate::{EditorImagePicker, RenderContext, TerminalImageCache};

const MAX_BLOCK_CELLS: usize = 256 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorPdfAction {
    Open,
    ZoomOut,
    ZoomIn,
    Rotate,
    Jump,
    Previous,
    Next,
}

impl EditorPdfAction {
    pub const ALL: [Self; 7] = [
        Self::Open,
        Self::ZoomOut,
        Self::ZoomIn,
        Self::Rotate,
        Self::Jump,
        Self::Previous,
        Self::Next,
    ];

    pub fn enabled(self, page: usize, total: usize, zoom: EditorPreviewZoom) -> bool {
        match self {
            Self::Open | Self::Rotate | Self::Jump => true,
            Self::ZoomOut => zoom.is_zoomed(),
            Self::ZoomIn => zoom.can_zoom_in(),
            Self::Previous => page > 0,
            Self::Next => page + 1 < total,
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::Open => "editor.pdf.open",
            Self::ZoomOut => "editor.pdf.zoom-out",
            Self::ZoomIn => "editor.pdf.zoom-in",
            Self::Rotate => "editor.pdf.rotate",
            Self::Jump => "editor.pdf.jump",
            Self::Previous => "editor.pdf.previous",
            Self::Next => "editor.pdf.next",
        }
    }

    fn label(self) -> String {
        match self {
            Self::Open => i18n::tr!("ui-editor-pdf-open"),
            Self::ZoomOut => i18n::tr!("ui-editor-preview-zoom-out"),
            Self::ZoomIn => i18n::tr!("ui-editor-preview-zoom-in"),
            Self::Rotate => i18n::tr!("ui-editor-pdf-rotate"),
            Self::Jump => i18n::tr!("ui-editor-pdf-jump"),
            Self::Previous => i18n::tr!("ui-editor-pdf-previous"),
            Self::Next => i18n::tr!("ui-editor-pdf-next"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorPdfViewModel {
    pub page: usize,
    pub page_id: u64,
    pub total: usize,
    pub focus: EditorPdfAction,
    pub zoom: EditorPreviewZoom,
    pub jump: Option<FormDialog>,
    pub image: Option<(u64, EditorPdfPageImage)>,
    pub image_zoom: Option<EditorPreviewZoom>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorPdfPageImage {
    pub raster: PdfPageImage,
    blocks: Arc<[u8]>,
}

impl EditorPdfPageImage {
    pub fn prepare(raster: PdfPageImage) -> Option<Self> {
        let [columns, rows] = raster.cells.map(u32::from);
        let image = ImageBuffer::<Rgba<u8>, _>::from_raw(
            raster.width,
            raster.height,
            raster.rgba.as_ref(),
        )?;
        let blocks = if u64::from(columns) * u64::from(rows) <= MAX_BLOCK_CELLS as u64 {
            Arc::from(
                crate::editor_media::resize_preview_pixels(
                    &image,
                    image.dimensions(),
                    [columns, rows * 2],
                )?
                .into_raw(),
            )
        } else {
            Arc::from([])
        };
        Some(Self { raster, blocks })
    }

    pub fn estimated_bytes(size: PdfPageSize) -> usize {
        let [width, height] = size.pixels.map(usize::from);
        let [columns, rows] = size.cells.map(usize::from);
        width * height * 4 + (columns * rows).min(MAX_BLOCK_CELLS) * 8
    }

    pub fn bytes(&self) -> usize {
        self.raster.rgba.len() + self.blocks.len()
    }

    fn draw_blocks(
        &self,
        frame: &mut Frame<'_>,
        area: Rect,
        [left, top]: [u32; 2],
        context: &RenderContext,
    ) {
        let [columns, rows] = self.raster.cells.map(usize::from);
        let (pixels, width, height) = if self.blocks.is_empty() {
            (
                &self.raster.rgba,
                self.raster.width as usize,
                self.raster.height as usize,
            )
        } else {
            (&self.blocks, columns, rows * 2)
        };
        let color = |x: usize, y: usize| {
            let x = ((2 * x + 1) * width) / (2 * columns);
            let y = ((2 * y + 1) * height) / (4 * rows);
            let offset = (y * width + x) * 4;
            Color::Rgb(pixels[offset], pixels[offset + 1], pixels[offset + 2])
        };
        let ansi = context.capabilities.color == crate::ColorCapability::Ansi;
        let colors = context.capabilities.terminal_colors;
        let visible = area.intersection(frame.area());
        for y in visible.y..visible.bottom() {
            for x in visible.x..visible.right() {
                let column = left as usize + usize::from(x - area.x);
                let half_row = 2 * (top as usize + usize::from(y - area.y));
                let (mut top, mut bottom) = (color(column, half_row), color(column, half_row + 1));
                if ansi {
                    top = colors.nearest_ansi(top);
                    bottom = colors.nearest_ansi(bottom);
                }
                let cell = &mut frame.buffer_mut()[(x, y)];
                cell.reset();
                cell.set_symbol(if top == bottom { " " } else { "▀" })
                    .set_fg(top)
                    .set_bg(bottom);
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorPdfLayout {
    title: Rect,
    page_count: Rect,
    buttons: Vec<(EditorPdfAction, Rect, bool)>,
}

impl EditorPdfLayout {
    pub fn button_at(&self, point: (u16, u16)) -> Option<ButtonRegion> {
        let (action, area, enabled) = self
            .buttons
            .iter()
            .find(|(_, area, _)| area.contains(point.into()))?;
        Some(ButtonRegion {
            id: action.id().into(),
            area: *area,
            disabled: !enabled,
        })
    }

    pub(super) fn action_at(&self, point: (u16, u16)) -> Option<EditorPdfAction> {
        self.buttons.iter().find_map(|(action, area, enabled)| {
            (*enabled && area.contains(point.into())).then_some(*action)
        })
    }
}

pub fn editor_pdf_canvas(area: Rect) -> Rect {
    let (footer, _) = footer_buttons(area);
    canvas_above_footer(area, footer)
}

fn canvas_above_footer(area: Rect, footer: Rect) -> Rect {
    let header = u16::from(area.height > 0);
    Rect::new(
        area.x,
        area.y.saturating_add(header),
        area.width,
        area.height - header - footer.height,
    )
}

fn action_width(action: EditorPdfAction) -> u16 {
    layout::to_u16(terminal_width(&action.label()) + 2)
}

fn footer_buttons(area: Rect) -> (Rect, Vec<(EditorPdfAction, Rect)>) {
    let actions = &EditorPdfAction::ALL[1..];
    let widths = actions
        .iter()
        .copied()
        .map(action_width)
        .collect::<Vec<_>>();
    let available = Rect::new(
        area.x,
        area.y,
        area.width,
        area.height
            .saturating_sub(2)
            .max(u16::from(area.height > 1)),
    );
    let positions = crate::right_aligned_actions(available, &widths);
    let height = positions
        .iter()
        .filter(|position| !position.is_empty())
        .map(|position| position.bottom() - area.y)
        .max()
        .unwrap_or(0);
    let y = area.bottom() - height;
    let buttons = actions
        .iter()
        .copied()
        .zip(positions)
        .filter(|(_, position)| !position.is_empty())
        .map(|(action, mut position)| {
            position.y = y + (position.y - area.y);
            (action, position)
        })
        .collect();
    (Rect::new(area.x, y, area.width, height), buttons)
}

pub(super) fn layout(area: Rect, pdf: &EditorPdfViewModel) -> EditorLayout {
    use EditorPdfAction::Open;

    let header = u16::from(area.height > 0);
    let (footer, buttons) = footer_buttons(area);
    let open_width = action_width(Open).min(area.width);
    let open = Rect::new(area.right() - open_width, area.y, open_width, header);
    let canvas = canvas_above_footer(area, footer);
    let count_end = buttons.first().map_or(area.right(), |(_, area)| area.x);

    EditorLayout {
        area,
        menu_bar: Rect::new(area.x, area.y, area.width, header),
        canvas_panel: canvas,
        canvas,
        status_bar: footer,
        pdf: Some(EditorPdfLayout {
            title: Rect::new(
                area.x,
                area.y,
                area.width.saturating_sub(open_width + 1),
                header,
            ),
            page_count: Rect::new(
                area.x,
                footer.y,
                count_end - area.x,
                u16::from(footer.height > 0),
            ),
            buttons: std::iter::once((Open, open, true))
                .chain(buttons.into_iter().map(|(action, area)| {
                    (action, area, action.enabled(pdf.page, pdf.total, pdf.zoom))
                }))
                .collect(),
        }),
        ..Default::default()
    }
}

pub(super) fn render(
    frame: &mut Frame<'_>,
    layout: &EditorLayout,
    model: &EditorViewModel,
    context: &RenderContext,
    images: &mut TerminalImageCache,
    picker: Option<&EditorImagePicker>,
) -> bool {
    let (Some(pdf), Some(pdf_layout)) = (&model.pdf, &layout.pdf) else {
        return false;
    };
    Surface::new().render_frame(frame, layout.menu_bar, context);
    Surface::new().render_frame(frame, layout.status_bar, context);
    let theme = context.compatibility_theme();
    let title = i18n::tr!(
        "ui-editor-read-only-title",
        title = terminal_safe_text(&model.file_name).into_owned()
    );
    let text = Style::default().fg(context.theme.text);
    frame.render_widget(Paragraph::new(title).style(text), pdf_layout.title);
    let count = i18n::tr!(
        "ui-editor-pdf-position",
        page = pdf.page + 1,
        total = pdf.total
    );
    let zoom = i18n::tr!(
        "ui-editor-preview-zoom-percent",
        percent = pdf.zoom.percent()
    );
    frame.render_widget(
        Paragraph::new(format!("{count}  {zoom}")).style(text),
        pdf_layout.page_count,
    );
    for &(action, area, enabled) in &pdf_layout.buttons {
        let mut button =
            Button::new(action.id(), format!(" {} ", action.label())).with_bracketed_label(false);
        button.state.focused = pdf.focus == action && pdf.jump.is_none();
        button.state.disabled = !enabled;
        button.render_inline_frame(frame, area, &theme);
    }

    let canvas = layout.canvas;
    let image_zoom = pdf.image_zoom.unwrap_or(pdf.zoom);
    if let Some((id, image)) = &pdf.image
        && image.raster.viewport == image_zoom.pdf_viewport(canvas)
    {
        let raster = &image.raster;
        let size = raster.cells.map(u32::from);
        let window = image_zoom.window(canvas, size);
        let placement = window.area;
        let fragment = crate::TerminalImageFragment {
            image_id: *id,
            width: raster.width,
            height: raster.height,
            rgba: raster.rgba.clone(),
            source: preview_pixel_crop(
                window.offset,
                placement,
                size,
                [raster.width, raster.height],
            ),
            padding: [0; 4],
            cell_size: raster.cell_size,
            area: placement,
            z_index: 0,
        };
        if let Some(picker) = picker {
            images.render(
                frame,
                canvas,
                &crate::CommandLineTerminalSnapshot {
                    columns: canvas.width,
                    rows: canvas.height,
                    cursor_style: Default::default(),
                    scrollback_rows: 0,
                    scrollback_offset: 0,
                    cells: Vec::new(),
                    images: vec![fragment],
                },
                Some(picker),
            );
        } else {
            images.clear();
            images.capture_preview(canvas, frame.area(), &fragment, pdf.page_id, None);
            image.draw_blocks(
                frame,
                Rect::new(
                    canvas.x + placement.x,
                    canvas.y + placement.y,
                    placement.width,
                    placement.height,
                ),
                window.offset,
                context,
            );
        }
        return true;
    }

    images.clear();
    if !canvas.is_empty() {
        let message = pdf
            .error
            .clone()
            .unwrap_or_else(|| i18n::tr!("ui-editor-pdf-rendering"));
        let area = Rect::new(canvas.x, canvas.y + canvas.height / 2, canvas.width, 1);
        frame.render_widget(
            Paragraph::new(message)
                .style(text)
                .alignment(HorizontalAlignment::Center),
            area,
        );
    }
    true
}
