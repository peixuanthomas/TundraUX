use app::editor::PdfPageImage;
use ratatui::layout::HorizontalAlignment;

use super::*;
use crate::components::{Button, ButtonRegion, Surface, terminal_width};
use crate::{EditorImagePicker, RenderContext, TerminalImageCache};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorPdfAction {
    Open,
    Previous,
    Next,
}

impl EditorPdfAction {
    pub const ALL: [Self; 3] = [Self::Open, Self::Previous, Self::Next];

    pub fn enabled(self, page: usize, total: usize) -> bool {
        match self {
            Self::Open => true,
            Self::Previous => page > 0,
            Self::Next => page + 1 < total,
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::Open => "editor.pdf.open",
            Self::Previous => "editor.pdf.previous",
            Self::Next => "editor.pdf.next",
        }
    }

    fn label(self) -> String {
        match self {
            Self::Open => i18n::tr!("ui-editor-pdf-open"),
            Self::Previous => i18n::tr!("ui-editor-pdf-previous"),
            Self::Next => i18n::tr!("ui-editor-pdf-next"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorPdfViewModel {
    pub page: usize,
    pub total: usize,
    pub focus: EditorPdfAction,
    pub image: Option<(u64, PdfPageImage)>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorPdfLayout {
    title: Rect,
    page_count: Rect,
    buttons: [(EditorPdfAction, Rect, bool); 3],
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

pub(super) fn layout(area: Rect, pdf: &EditorPdfViewModel) -> EditorLayout {
    use EditorPdfAction::{Next, Open, Previous};

    let header = u16::from(area.height > 0);
    let footer = u16::from(area.height > 1);
    let footer_y = area.bottom().saturating_sub(footer);
    let width = |action: EditorPdfAction| (terminal_width(&action.label()) + 2) as u16;
    let open_width = width(Open).min(area.width);
    let next_width = width(Next).min(area.width);
    let previous_width = width(Previous).min(area.width.saturating_sub(next_width + 1));
    let next = Rect::new(area.right() - next_width, footer_y, next_width, footer);
    let previous = Rect::new(
        next.x.saturating_sub(1 + previous_width).max(area.x),
        footer_y,
        previous_width,
        footer,
    );
    let open = Rect::new(area.right() - open_width, area.y, open_width, header);
    let canvas = Rect::new(
        area.x,
        area.y + header,
        area.width,
        area.height - header - footer,
    );

    EditorLayout {
        area,
        menu_bar: Rect::new(area.x, area.y, area.width, header),
        canvas_panel: canvas,
        canvas,
        status_bar: Rect::new(area.x, footer_y, area.width, footer),
        pdf: Some(EditorPdfLayout {
            title: Rect::new(
                area.x,
                area.y,
                area.width.saturating_sub(open_width + 1),
                header,
            ),
            page_count: Rect::new(area.x, footer_y, previous.x.saturating_sub(area.x), footer),
            buttons: [
                (Open, open, true),
                (Previous, previous, Previous.enabled(pdf.page, pdf.total)),
                (Next, next, Next.enabled(pdf.page, pdf.total)),
            ],
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
    frame.render_widget(Paragraph::new(count).style(text), pdf_layout.page_count);
    for (action, area, enabled) in pdf_layout.buttons {
        let mut button =
            Button::new(action.id(), format!(" {} ", action.label())).with_bracketed_label(false);
        button.state.focused = pdf.focus == action;
        button.state.disabled = !enabled;
        button.render_inline_frame(frame, area, &theme);
    }

    let canvas = layout.canvas;
    if let Some((id, image)) = &pdf.image
        && image.viewport == [canvas.width, canvas.height]
    {
        let [columns, rows] = image.cells;
        let placement = Rect::new(
            (canvas.width - columns) / 2,
            (canvas.height - rows) / 2,
            columns,
            rows,
        );
        let fragment = crate::TerminalImageFragment {
            image_id: *id,
            width: image.width,
            height: image.height,
            rgba: image.rgba.clone(),
            source: [0, 0, image.width, image.height],
            padding: [0; 4],
            cell_size: image.cell_size,
            area: placement,
            z_index: 0,
        };
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
            picker,
        );
        if picker.is_none() && context.capabilities.color == crate::ColorCapability::Ansi {
            let colors = context.capabilities.terminal_colors;
            let visible = Rect::new(
                canvas.x + placement.x,
                canvas.y + placement.y,
                columns,
                rows,
            )
            .intersection(frame.area());
            for y in visible.y..visible.bottom() {
                for x in visible.x..visible.right() {
                    let cell = &mut frame.buffer_mut()[(x, y)];
                    cell.fg = colors.nearest_ansi(cell.fg);
                    cell.bg = colors.nearest_ansi(cell.bg);
                }
            }
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
