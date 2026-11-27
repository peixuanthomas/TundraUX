use super::layout::to_u16;
use super::*;
use crate::components::{terminal_width, truncate_to_terminal_width};
use ratatui::layout::Size;
use std::ops::Range;

mod code_block;
mod preview;

use code_block::{
    code_block_heading, code_block_lines, code_tab_segments, code_tab_width, rich_code_block_lines,
};
#[cfg(test)]
pub(super) use preview::READ_ONLY_MEASURE_CALLS;
pub(super) use preview::ReadOnlyRichLayout;
use preview::TableWindow;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DisplayRun {
    pub(super) text: DisplayText,
    pub(super) style: EditorRenderSpan,
    pub(super) source: DisplaySource,
    pub(super) rich: DisplayRich,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum DisplayText {
    Owned(String),
    Shared(Arc<str>),
    /// A zero-copy slice of `EditorViewModel::source`.
    SourceRange(EditorSourceRange),
    Graphic(DisplayGraphic),
    CodeTab(usize),
}

impl DisplayText {
    pub(super) fn resolve<'a>(&'a self, source: Option<&'a str>) -> &'a str {
        match self {
            Self::Owned(text) => text,
            Self::Shared(text) => text,
            Self::SourceRange(range) => source
                .and_then(|source| source.get(range.start..range.end))
                .unwrap_or_default(),
            Self::Graphic(graphic) => graphic
                .fallback
                .lines()
                .nth(usize::from(graphic.row))
                .unwrap_or_default(),
            Self::CodeTab(_) => "\t",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DisplayGraphic {
    pub(super) graphic: EditorPreviewGraphic,
    pub(super) size: Size,
    pub(super) row: u16,
    pub(super) picture: bool,
    fallback: Arc<str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DisplaySource {
    Unmapped,
    Range(EditorSourceRange),
    Virtual(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DisplayRich {
    Unmapped,
    Range(RichRange),
    Virtual(RichPosition),
}

impl DisplayRun {
    pub(super) fn fixed_width(&self) -> Option<usize> {
        match &self.text {
            DisplayText::Graphic(graphic) => Some(usize::from(graphic.size.width)),
            DisplayText::CodeTab(width) => Some(*width),
            _ => None,
        }
    }

    fn graphic(
        graphic: &EditorPreviewGraphic,
        mut style: EditorRenderSpan,
        size: Size,
        picture: bool,
    ) -> Self {
        let fallback = std::mem::take(&mut style.text).into();
        style.media = None;
        Self {
            text: DisplayText::Graphic(DisplayGraphic {
                graphic: graphic.clone(),
                size,
                row: 0,
                picture,
                fallback,
            }),
            source: style
                .source_range
                .map_or(DisplaySource::Unmapped, DisplaySource::Range),
            rich: style
                .rich_range
                .map_or(DisplayRich::Unmapped, DisplayRich::Range),
            style,
        }
    }

    pub(super) fn unmapped(text: impl Into<String>, style: EditorRenderSpan) -> Self {
        Self {
            text: DisplayText::Owned(text.into()),
            style,
            source: DisplaySource::Unmapped,
            rich: DisplayRich::Unmapped,
        }
    }

    pub(super) fn virtual_text(
        text: impl Into<String>,
        style: EditorRenderSpan,
        byte_offset: Option<usize>,
    ) -> Self {
        Self {
            text: DisplayText::Owned(text.into()),
            style,
            source: byte_offset
                .map(DisplaySource::Virtual)
                .unwrap_or(DisplaySource::Unmapped),
            rich: DisplayRich::Unmapped,
        }
    }

    pub(super) fn source_range(range: EditorSourceRange) -> Self {
        Self {
            text: DisplayText::SourceRange(range),
            style: EditorRenderSpan::plain(""),
            source: DisplaySource::Range(range),
            rich: DisplayRich::Unmapped,
        }
    }

    pub(super) fn shared_source(text: Arc<str>, range: EditorSourceRange) -> Self {
        Self {
            text: DisplayText::Shared(text),
            style: EditorRenderSpan::plain(""),
            source: DisplaySource::Range(range),
            rich: DisplayRich::Unmapped,
        }
    }

    pub(super) fn with_virtual_rich(mut self, position: Option<RichPosition>) -> Self {
        if let Some(position) = position {
            self.rich = DisplayRich::Virtual(position);
        }
        self
    }

    /// Marks an otherwise zero-width display run as an editable Rich cursor
    /// boundary. This differs from a virtual decoration: the position is a
    /// real boundary in the document model even though it has no terminal
    /// cell of its own (for example, immediately before or after a soft
    /// break).
    pub(super) fn with_editable_rich_boundary(mut self, position: Option<RichPosition>) -> Self {
        if let Some(position) = position {
            self.rich = DisplayRich::Range(RichRange::between(position, position));
        }
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DisplayLine {
    pub(super) runs: Vec<DisplayRun>,
    pub(super) block_index: Option<usize>,
    pub(super) no_wrap: bool,
    /// Absolute display column of the first run. Non-zero only for a
    /// horizontally clipped Source viewport.
    pub(super) column_start: usize,
    pub(super) role: DisplayLineRole,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum DisplayLineRole {
    #[default]
    Normal,
    HeadingTop(u8),
    HeadingBottom(u8),
    Picture {
        row: u16,
        size: ratatui::layout::Size,
    },
}

impl DisplayLineRole {
    pub(super) const fn heading_level(self) -> Option<u8> {
        match self {
            Self::HeadingTop(level) | Self::HeadingBottom(level) => Some(level),
            Self::Normal | Self::Picture { .. } => None,
        }
    }
}

fn heading_uses_big_text(model: &EditorViewModel, block: &EditorRenderBlock) -> bool {
    if !model.text_sizing_protocol {
        return false;
    }
    let spans = match block {
        EditorRenderBlock::Heading { spans, .. }
        | EditorRenderBlock::PreviewHeading { spans, .. } => spans,
        _ => return false,
    };
    if model.canvas_read_only() {
        return media_cell_size(model).is_none() || !spans.iter().any(|span| span.media.is_some());
    }
    let Some(cursor) = model.rich_cursor else {
        return true;
    };

    // Multi-cell characters cannot expose exact per-grapheme selection and
    // caret styling. The heading currently being edited therefore stays in the
    // ordinary one-row editor representation; all inactive headings use the
    // terminal-native large-text representation.
    !spans.iter().any(|span| {
        span.rich_range.is_some_and(|range| {
            range.start.container_id == cursor.container_id
                && range.end.container_id == cursor.container_id
                && range.start.grapheme_offset <= cursor.grapheme_offset
                && cursor.grapheme_offset <= range.end.grapheme_offset
        })
    })
}

#[derive(Clone, Copy)]
struct BlockPresentation {
    big_heading: bool,
    wrap_cells: bool,
    cell_size: Option<[u16; 2]>,
    picture_bounds: super::picture::PictureBounds,
}

impl BlockPresentation {
    fn for_block(
        model: &EditorViewModel,
        block: &EditorRenderBlock,
        picture_bounds: super::picture::PictureBounds,
    ) -> Self {
        let big_heading = heading_uses_big_text(model, block);
        Self {
            big_heading,
            wrap_cells: model.canvas_read_only(),
            cell_size: (!big_heading).then(|| media_cell_size(model)).flatten(),
            picture_bounds,
        }
    }

    fn block_graphic(
        self,
        block: &EditorRenderBlock,
    ) -> Option<(&EditorPreviewGraphic, Size, bool)> {
        let (graphic, picture) = match block {
            EditorRenderBlock::PreviewHeading { graphic, .. } if self.cell_size.is_some() => {
                (graphic, false)
            }
            EditorRenderBlock::PreviewGraphic { graphic, kind, .. } => {
                let picture = *kind == EditorPreviewGraphicKind::Picture;
                if !picture && self.cell_size.is_none() {
                    return None;
                }
                (graphic, picture)
            }
            _ => return None,
        };
        Some((graphic, self.picture_bounds.fit(&graphic.image), picture))
    }
}

fn media_cell_size(model: &EditorViewModel) -> Option<[u16; 2]> {
    (model.image_protocol == EditorImageProtocolStatus::Available).then_some(model.image_cell_size)
}

fn inline_graphic_size(
    span: &EditorRenderSpan,
    width: usize,
    cell_size: Option<[u16; 2]>,
) -> Option<Size> {
    let graphic = span.media.as_ref()?;
    let cell_size = cell_size?;
    (width > 0).then(|| super::picture::bounds(width, usize::MAX, cell_size).fit(&graphic.image))
}

fn heading_wrap_width(width: usize, level: u8) -> usize {
    let (numerator, denominator) = crate::components::heading_size_ratio(level);
    width
        .max(1)
        .saturating_div(2)
        .saturating_mul(usize::from(denominator))
        .saturating_div(usize::from(numerator))
        .max(1)
}

struct DisplayColumnMeasure {
    column: usize,
    heading: Option<HeadingColumnMeasure>,
}

impl DisplayColumnMeasure {
    fn new(line: &DisplayLine) -> Self {
        Self {
            column: line.column_start,
            heading: line.role.heading_level().map(HeadingColumnMeasure::new),
        }
    }

    const fn column(&self) -> usize {
        self.column
    }

    fn push(&mut self, width: usize) -> usize {
        self.column = match self.heading.as_mut() {
            Some(heading) => heading.push(width),
            None => self.column.saturating_add(width),
        };
        self.column
    }
}

struct HeadingColumnMeasure {
    numerator: usize,
    denominator: usize,
    completed_width: usize,
    current_chunk_width: usize,
}

impl HeadingColumnMeasure {
    fn new(level: u8) -> Self {
        let (numerator, denominator) = crate::components::heading_size_ratio(level);
        Self {
            numerator: usize::from(numerator),
            denominator: usize::from(denominator),
            completed_width: 0,
            current_chunk_width: 0,
        }
    }

    fn push(&mut self, width: usize) -> usize {
        if width > 1 {
            self.flush();
            self.completed_width = self
                .completed_width
                .saturating_add(self.rendered_chunk_width(width));
            return self.completed_width;
        }
        if self.current_chunk_width.saturating_add(width) > self.denominator {
            self.flush();
        }
        self.current_chunk_width = self.current_chunk_width.saturating_add(width);
        self.completed_width
            .saturating_add(self.rendered_chunk_width(self.current_chunk_width))
    }

    fn flush(&mut self) {
        self.completed_width = self
            .completed_width
            .saturating_add(self.rendered_chunk_width(self.current_chunk_width));
        self.current_chunk_width = 0;
    }

    fn rendered_chunk_width(&self, width: usize) -> usize {
        if width == 0 {
            return 0;
        }
        width
            .saturating_mul(self.numerator)
            .div_ceil(self.denominator)
            .saturating_mul(2)
    }
}

/// Returns whether the Rich document fits without a vertical scrollbar.
///
/// This mirrors the line-counting semantics of [`block_lines`] without
/// constructing `DisplayRun`s. Wrapped blocks stop as soon as the supplied
/// height is exceeded, so a large document normally measures only a bounded
/// prefix before being flattened once at the final canvas width.
pub(super) fn rich_document_fits_height(
    model: &EditorViewModel,
    width: usize,
    height: usize,
) -> bool {
    let blocks = model.render_blocks();
    if blocks.is_empty() {
        return height >= 1;
    }

    let width = width.max(1);
    let mut line_count = 0usize;
    let picture_bounds = super::picture::bounds(width, height, model.image_cell_size);
    for (index, block) in blocks.iter().enumerate() {
        let remaining = height.saturating_sub(line_count);
        let Some(block_line_count) = rich_block_line_count_up_to_with_heading(
            block,
            width,
            remaining,
            model.table_widths_for_block(index, block.table_id()),
            BlockPresentation::for_block(model, block, picture_bounds),
        ) else {
            return false;
        };
        line_count = line_count.saturating_add(block_line_count);
    }
    line_count <= height
}

#[cfg(test)]
pub(super) fn rich_block_line_count_up_to(
    block: &EditorRenderBlock,
    width: usize,
    limit: usize,
) -> Option<usize> {
    rich_block_line_count_up_to_with_heading(
        block,
        width,
        limit,
        None,
        BlockPresentation {
            big_heading: false,
            wrap_cells: false,
            cell_size: None,
            picture_bounds: super::picture::bounds(width, usize::MAX, [8, 16]),
        },
    )
}

fn rich_block_line_count_up_to_with_heading(
    block: &EditorRenderBlock,
    width: usize,
    limit: usize,
    table_widths: Option<&[usize]>,
    presentation: BlockPresentation,
) -> Option<usize> {
    if let Some((_, size, _)) = presentation.block_graphic(block) {
        return fixed_line_count_up_to(usize::from(size.height), limit);
    }
    let wrap = |prefix: &str, spans: &[EditorRenderSpan]| {
        wrapped_line_count_up_to(prefix, spans, width, limit, presentation.cell_size)
    };
    match block {
        EditorRenderBlock::Picture { image, .. } => fixed_line_count_up_to(
            usize::from(presentation.picture_bounds.fit(image).height),
            limit,
        ),
        EditorRenderBlock::Paragraph(spans) => wrap("", spans),
        EditorRenderBlock::Heading { level, spans }
        | EditorRenderBlock::PreviewHeading { level, spans, .. }
            if presentation.big_heading =>
        {
            let scaled_width = heading_wrap_width(width, (*level).clamp(1, 6));
            let line_limit = limit / 2;
            wrapped_line_count_up_to("", spans, scaled_width, line_limit, None)
                .and_then(|lines| lines.checked_mul(2))
                .filter(|lines| *lines <= limit)
        }
        EditorRenderBlock::Heading { spans, .. }
        | EditorRenderBlock::PreviewHeading { spans, .. } => wrap("", spans),
        EditorRenderBlock::BulletListItem {
            depth,
            checked,
            spans,
        } => {
            let marker = match checked {
                Some(true) => "☒ ",
                Some(false) => "☐ ",
                None => "• ",
            };
            let prefix = format!("{}{}", "  ".repeat(usize::from(*depth)), marker);
            wrap(&prefix, spans)
        }
        EditorRenderBlock::OrderedListItem {
            depth,
            number,
            spans,
        } => {
            let prefix = format!("{}{}. ", "  ".repeat(usize::from(*depth)), number);
            wrap(&prefix, spans)
        }
        EditorRenderBlock::Quote { depth, spans } => {
            let prefix = "│ ".repeat(usize::from((*depth).max(1)));
            wrap(&prefix, spans)
        }
        EditorRenderBlock::Footnote { label, spans } => {
            let prefix = format!("[^{label}] ");
            wrap(&prefix, spans)
        }
        EditorRenderBlock::CodeBlock { lines, .. } => {
            fixed_line_count_up_to(lines.len().saturating_add(2), limit)
        }
        EditorRenderBlock::RichCodeBlock { lines, .. } => {
            fixed_line_count_up_to(lines.len().saturating_add(2), limit)
        }
        EditorRenderBlock::Table { header, rows, .. }
        | EditorRenderBlock::RichTable { header, rows, .. } => {
            let has_columns = !header.is_empty() || rows.iter().any(|row| !row.is_empty());
            let minimum_lines = if !has_columns {
                1
            } else {
                rows.len()
                    .saturating_add(2)
                    .saturating_add(usize::from(!header.is_empty()) * 2)
            };
            fixed_line_count_up_to(minimum_lines, limit)?;
            if has_columns && presentation.wrap_cells {
                let widths = table_column_widths_cancellable(
                    header,
                    rows,
                    table_widths,
                    presentation.cell_size,
                    &|| false,
                )
                .unwrap();
                let mut lines = 2 + usize::from(!header.is_empty());
                let header = (!header.is_empty()).then_some(header);
                for row in header.into_iter().chain(rows) {
                    let remaining = limit.checked_sub(lines)?;
                    let row_height =
                        wrapped_table_row_height(row, &widths, remaining, presentation.cell_size)?;
                    lines = lines.saturating_add(row_height);
                }
                return fixed_line_count_up_to(lines, limit);
            }
            Some(minimum_lines)
        }
        EditorRenderBlock::RawHtml(html) => fixed_line_count_up_to(source_line_count(html), limit),
        EditorRenderBlock::PreviewGraphic {
            fallback, range, ..
        } => wrap("", &[graphic_fallback(fallback.clone(), *range)]),
        EditorRenderBlock::HorizontalRule
        | EditorRenderBlock::Image { .. }
        | EditorRenderBlock::Blank => fixed_line_count_up_to(1, limit),
    }
}

pub(super) fn fixed_line_count_up_to(line_count: usize, limit: usize) -> Option<usize> {
    (line_count <= limit).then_some(line_count)
}

fn wrapped_table_row_height(
    row: &[EditorTableCell],
    widths: &[usize],
    limit: usize,
    cell_size: Option<[u16; 2]>,
) -> Option<usize> {
    row.iter()
        .zip(widths)
        .try_fold(1, |height, (cell, &width)| {
            Some(height.max(wrapped_line_count_up_to(
                "",
                &cell.spans,
                width,
                limit,
                cell_size,
            )?))
        })
}

fn printable_ascii(text: &str) -> bool {
    text.bytes().all(|byte| (b' '..=b'~').contains(&byte))
}

/// Counts the lines produced by [`wrap_runs`] without creating one owned
/// `DisplayRun` per grapheme. `None` means the count exceeded `limit`.
fn wrapped_line_count_up_to(
    prefix: &str,
    spans: &[EditorRenderSpan],
    width: usize,
    limit: usize,
    cell_size: Option<[u16; 2]>,
) -> Option<usize> {
    let prefix_width = terminal_width(terminal_safe_text(prefix).as_ref());
    let mut measure = WrappedLineMeasure {
        width,
        limit,
        prefix_width,
        line_count: 0,
        current_width: prefix_width,
        current_height: 1,
        current_has_runs: !prefix.is_empty(),
        continuation_has_runs: prefix_width > 0,
    };

    for span in spans {
        if let Some(size) = inline_graphic_size(span, width.saturating_sub(prefix_width), cell_size)
        {
            if !measure.push_item(usize::from(size.width), usize::from(size.height)) {
                return None;
            }
            continue;
        }
        // `mapped_text_runs` emits one empty run for an empty span. It affects
        // whether a final empty visual line exists even though it has no width.
        if span.text.is_empty() {
            measure.current_has_runs = true;
            continue;
        }
        if printable_ascii(&span.text) {
            if !measure.push_ascii(span.text.len()) {
                return None;
            }
            continue;
        }

        let bytes = span.text.as_bytes();
        let mut segment_start = 0usize;
        let mut index = 0usize;
        while index < bytes.len() {
            let newline_len = match bytes[index] {
                b'\r' if bytes.get(index + 1) == Some(&b'\n') => 2,
                b'\r' | b'\n' => 1,
                _ => {
                    index += 1;
                    continue;
                }
            };
            if !measure.push_segment(&span.text[segment_start..index]) {
                return None;
            }
            let mapped_boundary = span.source_range.is_some() || span.rich_range.is_some();
            if !measure.push_newline(mapped_boundary) {
                return None;
            }
            index += newline_len;
            segment_start = index;
        }
        if !measure.push_segment(&span.text[segment_start..]) {
            return None;
        }
        if segment_start == 0 {
            measure.current_has_runs = true;
        }
    }

    measure.finish()
}

struct WrappedLineMeasure {
    width: usize,
    limit: usize,
    prefix_width: usize,
    line_count: usize,
    current_width: usize,
    current_height: usize,
    current_has_runs: bool,
    continuation_has_runs: bool,
}

impl WrappedLineMeasure {
    fn push_ascii(&mut self, length: usize) -> bool {
        let capacity = self.width.saturating_sub(self.prefix_width).max(1);
        let room = capacity.saturating_sub(self.current_width - self.prefix_width);
        if length <= room {
            self.current_width += length;
        } else {
            let remaining = length - room;
            self.line_count = self
                .line_count
                .saturating_add(self.current_height)
                .saturating_add((remaining - 1) / capacity);
            self.current_width = self.prefix_width + (remaining - 1) % capacity + 1;
            self.current_height = 1;
        }
        self.current_has_runs = true;
        self.line_count <= self.limit
    }

    pub(super) fn push_segment(&mut self, segment: &str) -> bool {
        if segment.is_empty() {
            return true;
        }
        let span = Span::raw(segment);
        for grapheme in span.styled_graphemes(Style::default()) {
            let run_width = terminal_width(terminal_safe_text(grapheme.symbol).as_ref());
            if !self.push_item(run_width, 1) {
                return false;
            }
        }
        true
    }

    fn push_item(&mut self, width: usize, height: usize) -> bool {
        if self.current_width > self.prefix_width
            && self.current_width.saturating_add(width) > self.width
            && !self.push_line()
        {
            return false;
        }
        self.current_width = self.current_width.saturating_add(width);
        self.current_height = self.current_height.max(height);
        self.current_has_runs = true;
        true
    }

    pub(super) fn push_newline(&mut self, mapped_boundary: bool) -> bool {
        if !self.push_line() {
            return false;
        }
        self.current_width = self.prefix_width;
        self.current_has_runs = self.continuation_has_runs || mapped_boundary;
        true
    }

    pub(super) fn push_line(&mut self) -> bool {
        self.line_count = self.line_count.saturating_add(self.current_height);
        if self.line_count > self.limit {
            return false;
        }
        self.current_width = self.prefix_width;
        self.current_height = 1;
        self.current_has_runs = self.continuation_has_runs;
        true
    }

    pub(super) fn finish(mut self) -> Option<usize> {
        if (self.current_has_runs || self.line_count == 0) && !self.push_line() {
            return None;
        }
        Some(self.line_count)
    }
}

#[cfg(test)]
pub(super) struct ThreadLocalCounter;

#[cfg(test)]
static RICH_FLATTEN_COUNTS: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<std::thread::ThreadId, usize>>,
> = std::sync::LazyLock::new(|| std::sync::Mutex::new(std::collections::HashMap::new()));

#[cfg(test)]
impl ThreadLocalCounter {
    pub(super) fn with<R>(&self, operation: impl FnOnce(&std::cell::Cell<usize>) -> R) -> R {
        let thread_id = std::thread::current().id();
        let mut counts = RICH_FLATTEN_COUNTS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let count = std::cell::Cell::new(counts.get(&thread_id).copied().unwrap_or_default());
        let result = operation(&count);
        counts.insert(thread_id, count.get());
        result
    }
}

#[cfg(test)]
pub(super) static RICH_FLATTEN_CALL_COUNT: ThreadLocalCounter = ThreadLocalCounter;

pub(super) fn flatten_rich_document(
    model: &EditorViewModel,
    width: usize,
    height: usize,
) -> Vec<DisplayLine> {
    #[cfg(test)]
    RICH_FLATTEN_CALL_COUNT.with(|count| count.set(count.get().saturating_add(1)));

    let width = width.max(1);
    let mut output = Vec::new();
    let bounds = super::picture::bounds(width, height, model.image_cell_size);
    for block_index in 0..model.render_blocks().len() {
        output.extend(materialize_rich_block(
            model,
            block_index,
            width,
            bounds,
            None,
        ));
    }
    if output.is_empty() {
        output.push(empty_display_line(None));
    }
    output
}

fn materialize_rich_block(
    model: &EditorViewModel,
    index: usize,
    width: usize,
    picture_bounds: super::picture::PictureBounds,
    table_window: Option<TableWindow<'_>>,
) -> Vec<DisplayLine> {
    let block = &model.render_blocks()[index];
    block_lines_with_heading(
        block,
        index,
        width,
        model.block_sources.get(index).copied(),
        model.table_widths_for_block(index, block.table_id()),
        model.source.as_deref(),
        BlockPresentation::for_block(model, block, picture_bounds),
        table_window,
    )
}

#[cfg(test)]
pub(super) fn block_lines(
    block: &EditorRenderBlock,
    block_index: usize,
    width: usize,
    block_source: Option<EditorBlockSourceMap>,
    table_widths: Option<&[usize]>,
    source: Option<&str>,
) -> Vec<DisplayLine> {
    block_lines_with_heading(
        block,
        block_index,
        width,
        block_source,
        table_widths,
        source,
        BlockPresentation {
            big_heading: false,
            wrap_cells: false,
            cell_size: None,
            picture_bounds: super::picture::bounds(width, usize::MAX, [8, 16]),
        },
        None,
    )
}

fn block_lines_with_heading(
    block: &EditorRenderBlock,
    block_index: usize,
    width: usize,
    block_source: Option<EditorBlockSourceMap>,
    table_widths: Option<&[usize]>,
    source: Option<&str>,
    presentation: BlockPresentation,
    table_window: Option<TableWindow<'_>>,
) -> Vec<DisplayLine> {
    let anchor = block_source.map(EditorBlockSourceMap::anchor);
    if let Some((graphic, size, picture)) = presentation.block_graphic(block) {
        return block_graphic_lines(block, block_index, width, graphic, size, picture);
    }
    let wrap = |prefix, spans: &[EditorRenderSpan], max_width, cell_size| {
        wrap_runs(
            prefix,
            spans,
            max_width,
            block_index,
            false,
            source,
            cell_size,
        )
    };
    match block {
        EditorRenderBlock::Picture { image, .. } => {
            let size = presentation.picture_bounds.fit(image);
            (0..size.height)
                .map(|row| DisplayLine {
                    role: DisplayLineRole::Picture { row, size },
                    ..empty_display_line(Some(block_index))
                })
                .collect()
        }
        EditorRenderBlock::Paragraph(spans) => {
            wrap(Vec::new(), spans, width, presentation.cell_size)
        }
        EditorRenderBlock::Heading { level, spans }
        | EditorRenderBlock::PreviewHeading { level, spans, .. } => {
            let level = (*level).clamp(1, 6);
            let styled: Vec<_> = spans
                .iter()
                .cloned()
                .map(|mut span| {
                    span.bold = true;
                    span.color = EditorSpanColor::Normal;
                    span
                })
                .collect();
            let wrap_width = if presentation.big_heading {
                heading_wrap_width(width, level)
            } else {
                width
            };
            let lines = wrap(Vec::new(), &styled, wrap_width, presentation.cell_size);
            if presentation.big_heading {
                lines
                    .into_iter()
                    .flat_map(|mut line| {
                        line.role = DisplayLineRole::HeadingTop(level);
                        let mut bottom = line.clone();
                        bottom.role = DisplayLineRole::HeadingBottom(level);
                        [line, bottom]
                    })
                    .collect()
            } else {
                lines
            }
        }
        EditorRenderBlock::BulletListItem {
            depth,
            checked,
            spans,
        } => {
            let marker = match checked {
                Some(true) => "☒ ",
                Some(false) => "☐ ",
                None => "• ",
            };
            let prefix = format!("{}{}", "  ".repeat(usize::from(*depth)), marker);
            wrap(
                vec![DisplayRun::virtual_text(prefix, accent_span(), anchor)],
                spans,
                width,
                presentation.cell_size,
            )
        }
        EditorRenderBlock::OrderedListItem {
            depth,
            number,
            spans,
        } => {
            let prefix = format!("{}{}. ", "  ".repeat(usize::from(*depth)), number);
            wrap(
                vec![DisplayRun::virtual_text(prefix, accent_span(), anchor)],
                spans,
                width,
                presentation.cell_size,
            )
        }
        EditorRenderBlock::Quote { depth, spans } => {
            let prefix = "│ ".repeat(usize::from((*depth).max(1)));
            let quote_spans: Vec<_> = spans
                .iter()
                .cloned()
                .map(|mut span| {
                    span.italic = true;
                    span
                })
                .collect();
            wrap(
                vec![DisplayRun::virtual_text(prefix, accent_span(), anchor)],
                &quote_spans,
                width,
                presentation.cell_size,
            )
        }
        EditorRenderBlock::CodeBlock { language, lines } => {
            let line_ranges = code_line_source_ranges(lines, block_source, source);
            let end_anchor = block_source
                .and_then(|mapping| mapping.content_range)
                .map(|range| range.end)
                .or(anchor);
            let runs = lines.iter().zip(line_ranges).map(|(line, range)| {
                mapped_text_runs(line, EditorRenderSpan::code(""), range, source)
            });
            code_block_lines(language.as_deref(), block_index, anchor, end_anchor, runs)
        }
        EditorRenderBlock::RichCodeBlock { language, lines } => rich_code_block_lines(
            language.as_deref(),
            block_index,
            lines,
            anchor,
            source,
            0..lines.len().saturating_add(2),
        ),
        EditorRenderBlock::Table {
            header,
            rows,
            alignments,
        }
        | EditorRenderBlock::RichTable {
            table_id: _,
            header,
            rows,
            alignments,
        } => table_lines(
            header,
            rows,
            alignments,
            table_widths,
            block_index,
            anchor,
            source,
            presentation.wrap_cells,
            presentation.cell_size,
            table_window,
        ),
        EditorRenderBlock::HorizontalRule => vec![DisplayLine {
            runs: vec![DisplayRun::virtual_text(
                "─".repeat(width),
                accent_span(),
                anchor,
            )],
            block_index: Some(block_index),
            no_wrap: true,
            column_start: 0,
            role: DisplayLineRole::Normal,
        }],
        EditorRenderBlock::RawHtml(html) => {
            let block_range = block_source.map(|mapping| mapping.source_range);
            let exact_source_start = block_range
                .filter(|range| {
                    source.and_then(|source| source.get(range.start..range.end)) == Some(html)
                })
                .map(|range| range.start);
            source_display_line_ranges(html)
                .into_iter()
                .enumerate()
                .map(|(line_index, local_range)| {
                    let mapped_range = exact_source_start.map(|start| {
                        EditorSourceRange::new(
                            start.saturating_add(local_range.start),
                            start.saturating_add(local_range.end),
                        )
                    });
                    let line_anchor = mapped_range.map(|range| range.start).or(anchor);
                    let mut runs = Vec::new();
                    if line_index == 0 {
                        runs.push(DisplayRun::virtual_text(
                            "HTML ",
                            warning_span(),
                            line_anchor,
                        ));
                    }
                    runs.extend(mapped_text_runs(
                        html.get(local_range.start..local_range.end)
                            .unwrap_or_default(),
                        warning_span(),
                        mapped_range,
                        source,
                    ));
                    DisplayLine {
                        runs,
                        block_index: Some(block_index),
                        no_wrap: true,
                        column_start: 0,
                        role: DisplayLineRole::Normal,
                    }
                })
                .collect()
        }
        EditorRenderBlock::Image { markdown } => vec![DisplayLine {
            runs: mapped_text_runs(
                markdown,
                muted_span(),
                block_source.map(|mapping| mapping.source_range),
                source,
            ),
            block_index: Some(block_index),
            no_wrap: true,
            column_start: 0,
            role: DisplayLineRole::Normal,
        }],
        EditorRenderBlock::PreviewGraphic {
            fallback, range, ..
        } => wrap(
            Vec::new(),
            &[graphic_fallback(fallback.clone(), *range)],
            width,
            None,
        ),
        EditorRenderBlock::Footnote { label, spans } => wrap(
            vec![DisplayRun::virtual_text(
                format!("[^{label}] "),
                accent_span(),
                anchor,
            )],
            spans,
            width,
            presentation.cell_size,
        ),
        EditorRenderBlock::Blank => vec![empty_display_line_at(Some(block_index), anchor)],
    }
}

fn graphic_fallback(text: String, range: Option<RichRange>) -> EditorRenderSpan {
    EditorRenderSpan {
        rich_range: range,
        ..EditorRenderSpan::plain(text)
    }
}

fn block_graphic_lines(
    block: &EditorRenderBlock,
    block_index: usize,
    width: usize,
    graphic: &EditorPreviewGraphic,
    size: Size,
    picture: bool,
) -> Vec<DisplayLine> {
    let (span, left) = match block {
        EditorRenderBlock::PreviewHeading { spans, .. } => {
            let mut ranges = spans.iter().filter_map(|span| span.rich_range);
            let range = ranges
                .next()
                .map(|first| RichRange::between(first.start, ranges.last().unwrap_or(first).end));
            let text = spans
                .iter()
                .map(|span| span.text.as_str())
                .collect::<String>();
            (
                EditorRenderSpan {
                    rich_range: range,
                    ..EditorRenderSpan::strong(text)
                },
                0,
            )
        }
        EditorRenderBlock::PreviewGraphic {
            fallback, range, ..
        } => (
            graphic_fallback(fallback.clone(), *range),
            width.saturating_sub(usize::from(size.width)) / 2,
        ),
        _ => unreachable!(),
    };
    let runs = vec![DisplayRun::graphic(graphic, span, size, picture)];
    let mut lines = Vec::with_capacity(usize::from(size.height));
    append_wrapped_line(&mut lines, runs, block_index, false, left);
    lines
}

fn table_lines(
    header: &[EditorTableCell],
    rows: &[Vec<EditorTableCell>],
    alignments: &[EditorTableAlignment],
    requested_widths: Option<&[usize]>,
    block_index: usize,
    anchor: Option<usize>,
    source: Option<&str>,
    wrap_cells: bool,
    cell_size: Option<[u16; 2]>,
    window: Option<TableWindow<'_>>,
) -> Vec<DisplayLine> {
    let measured_widths;
    let widths = if let Some(window) = &window {
        window.metrics.widths.as_slice()
    } else {
        measured_widths =
            table_column_widths_cancellable(header, rows, requested_widths, cell_size, &|| false)
                .unwrap();
        &measured_widths
    };
    if widths.is_empty() {
        return vec![empty_display_line_at(Some(block_index), anchor)];
    }
    let rich_anchor = std::iter::once(header)
        .chain(rows.iter().map(Vec::as_slice))
        .flat_map(|row| row.iter())
        .find_map(|cell| table_cell_rich_range(cell).map(|range| range.start));
    let first_row = if header.is_empty() {
        rows.first().map(Vec::as_slice).unwrap_or_default()
    } else {
        header
    };
    let context = TableLineContext {
        widths,
        block_index,
        anchor,
        rich_anchor,
    };
    let append_row = |output: &mut Vec<DisplayLine>,
                      row: &[EditorTableCell],
                      header: bool,
                      within: std::ops::Range<usize>| {
        if within.is_empty() {
            return;
        }
        let mut wrapped_cells = Vec::new();
        if wrap_cells {
            wrapped_cells.extend(row.iter().zip(widths).map(|(cell, &width)| {
                wrap_runs(
                    Vec::new(),
                    &cell.spans,
                    width,
                    block_index,
                    true,
                    source,
                    cell_size,
                )
                .into_iter()
            }));
        }
        let height = wrapped_cells
            .iter()
            .map(ExactSizeIterator::len)
            .max()
            .unwrap_or(1);
        if within.start > 0 {
            for lines in &mut wrapped_cells {
                lines.nth(within.start - 1);
            }
        }
        output.extend((within.start..height.min(within.end)).map(|_| {
            table_row_line(
                row,
                alignments,
                header,
                source,
                &context,
                &mut wrapped_cells,
            )
        }));
    };
    let show = |line| {
        window
            .as_ref()
            .is_none_or(|window| window.lines.contains(&line))
    };
    let mut output = Vec::new();
    if show(0) {
        output.push(table_border_line(
            ('┌', '┬', '┐'),
            first_row,
            false,
            &context,
        ));
    }
    if !header.is_empty() {
        let within = window.as_ref().map_or(0..usize::MAX, |window| {
            preview::intersect_lines(&window.lines, 1, window.metrics.header_end)
        });
        append_row(&mut output, header, true, within);
        if window
            .as_ref()
            .is_none_or(|window| show(window.metrics.header_end))
        {
            output.push(table_border_line(('├', '┼', '┤'), header, false, &context));
        }
    }
    if let Some(window) = &window {
        let metrics = window.metrics;
        let first_index = metrics
            .row_ends
            .partition_point(|&end| end <= window.lines.start);
        for index in first_index..rows.len() {
            let start = if index == 0 {
                metrics.header_end + usize::from(!header.is_empty())
            } else {
                metrics.row_ends[index - 1]
            };
            if start >= window.lines.end {
                break;
            }
            let within = preview::intersect_lines(&window.lines, start, metrics.row_ends[index]);
            append_row(&mut output, &rows[index], false, within);
        }
    } else {
        for row in rows {
            append_row(&mut output, row, false, 0..usize::MAX);
        }
    }
    if window
        .as_ref()
        .is_none_or(|window| show(window.metrics.line_count - 1))
    {
        let last_row = rows.last().map(Vec::as_slice).unwrap_or(first_row);
        output.push(table_border_line(
            ('└', '┴', '┘'),
            last_row,
            false,
            &context,
        ));
    }
    output
}

pub(super) struct TableLineContext<'a> {
    widths: &'a [usize],
    block_index: usize,
    anchor: Option<usize>,
    rich_anchor: Option<RichPosition>,
}

fn table_row_line(
    row: &[EditorTableCell],
    alignments: &[EditorTableAlignment],
    header: bool,
    source: Option<&str>,
    context: &TableLineContext<'_>,
    wrapped_cells: &mut [std::vec::IntoIter<DisplayLine>],
) -> DisplayLine {
    let row_anchor = row
        .iter()
        .find_map(|cell| table_cell_rich_range(cell).map(|range| range.start))
        .or(context.rich_anchor);
    let mut runs = vec![
        DisplayRun::virtual_text("│", table_span(header), context.anchor)
            .with_virtual_rich(row_anchor),
    ];
    for (column, width) in context.widths.iter().enumerate() {
        let cell = row.get(column);
        let cell_range = cell.and_then(table_cell_source_range);
        let start = cell_range.map(|range| range.start);
        let end = cell_range.map(|range| range.end);
        let rich_range = cell.and_then(table_cell_rich_range);
        let mut content = if let Some(lines) = wrapped_cells.get_mut(column) {
            lines.next().map(|line| line.runs).unwrap_or_default()
        } else {
            cell.into_iter()
                .flat_map(|cell| &cell.spans)
                .flat_map(|span| {
                    mapped_text_runs(&span.text, span.clone(), span.source_range, source)
                })
                .collect()
        };
        for run in &mut content {
            run.style.color = EditorSpanColor::Accent;
            run.style.bold |= header;
        }
        let rich_start = content
            .iter()
            .find_map(display_run_rich_start)
            .or(rich_range.map(|range| range.start))
            .or(row_anchor);
        let rich_end = content
            .iter()
            .rev()
            .find_map(display_run_rich_end)
            .or(rich_range.map(|range| range.end))
            .or(rich_start);
        let (fitted, used) = fit_table_content(
            content,
            *width,
            end.or(start).or(context.anchor),
            rich_end,
            header,
        );
        let remaining = width.saturating_sub(used);
        let (aligned_left, aligned_right) =
            match alignments.get(column).copied().unwrap_or_default() {
                EditorTableAlignment::Left => (0, remaining),
                EditorTableAlignment::Center => {
                    (remaining / 2, remaining.saturating_sub(remaining / 2))
                }
                EditorTableAlignment::Right => (remaining, 0),
            };
        runs.push(table_padding(
            " ".repeat(aligned_left.saturating_add(1)),
            start,
            context.anchor,
            rich_start,
            header,
        ));
        runs.extend(fitted);
        runs.push(table_padding(
            " ".repeat(aligned_right.saturating_add(1)),
            end.or(start),
            context.anchor,
            rich_end,
            header,
        ));
        runs.push(
            DisplayRun::virtual_text("│", table_span(header), end.or(start).or(context.anchor))
                .with_virtual_rich(rich_end),
        );
    }
    DisplayLine {
        runs,
        block_index: Some(context.block_index),
        no_wrap: true,
        column_start: 0,
        role: DisplayLineRole::Normal,
    }
}

pub(super) fn table_border_line(
    glyphs: (char, char, char),
    cells: &[EditorTableCell],
    header: bool,
    context: &TableLineContext<'_>,
) -> DisplayLine {
    let (left, middle, right) = glyphs;
    let first_anchor = cells
        .first()
        .and_then(table_cell_rich_range)
        .map(|range| range.start)
        .or(context.rich_anchor);
    let mut runs = vec![
        DisplayRun::virtual_text(left.to_string(), table_span(header), context.anchor)
            .with_virtual_rich(first_anchor),
    ];
    for (column, width) in context.widths.iter().copied().enumerate() {
        let cell_range = cells.get(column).and_then(table_cell_rich_range);
        let start = cell_range.map(|range| range.start).or(context.rich_anchor);
        let end = cell_range.map(|range| range.end).or(start);
        runs.push(
            DisplayRun::virtual_text(
                "─".repeat(width.saturating_add(2)),
                table_span(header),
                context.anchor,
            )
            .with_virtual_rich(start),
        );
        let delimiter = if column + 1 == context.widths.len() {
            right
        } else {
            middle
        };
        runs.push(
            DisplayRun::virtual_text(delimiter.to_string(), table_span(header), context.anchor)
                .with_virtual_rich(end),
        );
    }
    DisplayLine {
        runs,
        block_index: Some(context.block_index),
        no_wrap: true,
        column_start: 0,
        role: DisplayLineRole::Normal,
    }
}

pub(super) fn table_span(header: bool) -> EditorRenderSpan {
    let mut style = EditorRenderSpan::plain("");
    style.color = EditorSpanColor::Accent;
    style.bold = header;
    style
}

pub(super) fn table_padding(
    text: String,
    byte_offset: Option<usize>,
    fallback: Option<usize>,
    rich_position: Option<RichPosition>,
    header: bool,
) -> DisplayRun {
    match byte_offset {
        Some(byte_offset) => DisplayRun {
            text: DisplayText::Owned(text),
            style: table_span(header),
            source: DisplaySource::Range(EditorSourceRange::new(byte_offset, byte_offset)),
            rich: rich_position
                .map(DisplayRich::Virtual)
                .unwrap_or(DisplayRich::Unmapped),
        },
        None => DisplayRun::virtual_text(text, table_span(header), fallback)
            .with_virtual_rich(rich_position),
    }
}

pub(super) fn table_cell_rich_range(cell: &EditorTableCell) -> Option<RichRange> {
    let mut ranges = cell.spans.iter().filter_map(|span| span.rich_range);
    let first = ranges.next()?;
    let mut start = first.start;
    let mut end = first.end;
    for range in ranges {
        if range.start.container_id != start.container_id
            || range.end.container_id != start.container_id
        {
            return Some(RichRange::between(first.start, first.end));
        }
        start.grapheme_offset = start.grapheme_offset.min(range.start.grapheme_offset);
        end.grapheme_offset = end.grapheme_offset.max(range.end.grapheme_offset);
    }
    Some(RichRange::between(start, end))
}

pub(super) fn table_cell_source_range(cell: &EditorTableCell) -> Option<EditorSourceRange> {
    Some(EditorSourceRange::new(
        cell.spans
            .iter()
            .filter_map(|span| span.source_range)
            .map(|range| range.start)
            .min()?,
        cell.spans
            .iter()
            .filter_map(|span| span.source_range)
            .map(|range| range.end)
            .max()?,
    ))
}

pub(super) fn fit_table_content(
    runs: Vec<DisplayRun>,
    width: usize,
    overflow_anchor: Option<usize>,
    rich_overflow_anchor: Option<RichPosition>,
    header: bool,
) -> (Vec<DisplayRun>, usize) {
    let used = runs_width(&runs);
    if used <= width {
        return (runs, used);
    }
    if width == 0 {
        return (Vec::new(), 0);
    }
    let content_width = width.saturating_sub(1);
    let mut fitted = Vec::new();
    let mut fitted_width = 0usize;
    for run in runs {
        let run_width = display_run_width(&run);
        if fitted_width.saturating_add(run_width) > content_width {
            break;
        }
        fitted_width = fitted_width.saturating_add(run_width);
        fitted.push(run);
    }
    fitted.push(
        DisplayRun::virtual_text("…", table_span(header), overflow_anchor)
            .with_virtual_rich(rich_overflow_anchor),
    );
    (fitted, width)
}

pub(super) fn table_column_widths(
    header: &[EditorTableCell],
    rows: &[Vec<EditorTableCell>],
    requested: Option<&[usize]>,
) -> Vec<usize> {
    table_column_widths_cancellable(header, rows, requested, None, &|| false).unwrap()
}

fn table_column_widths_cancellable(
    header: &[EditorTableCell],
    rows: &[Vec<EditorTableCell>],
    requested: Option<&[usize]>,
    cell_size: Option<[u16; 2]>,
    cancelled: &impl Fn() -> bool,
) -> Option<Vec<usize>> {
    let mut widths = vec![1usize; header.len()];
    for row in std::iter::once(header).chain(rows.iter().map(Vec::as_slice)) {
        if cancelled() {
            return None;
        }
        if row.len() > widths.len() {
            widths.resize(row.len(), 1);
        }
        for (index, cell) in row.iter().enumerate() {
            if widths[index] == 24 {
                continue;
            }
            let width = if cell_size.is_some() && cell.spans.iter().any(|span| span.media.is_some())
            {
                cell.spans
                    .iter()
                    .map(|span| {
                        inline_graphic_size(span, 24, cell_size).map_or_else(
                            || measured_text_width(&span.text),
                            |size| usize::from(size.width),
                        )
                    })
                    .fold(0, usize::saturating_add)
            } else {
                let text = match cell.spans.as_slice() {
                    [] => continue,
                    [span] => Cow::Borrowed(span.text.as_str()),
                    _ => Cow::Owned(cell_text(cell)),
                };
                measured_text_width(&text)
            };
            widths[index] = widths[index].max(width.min(24));
        }
    }
    if let Some(requested) = requested {
        for (width, requested) in widths.iter_mut().zip(requested.iter().copied()) {
            if requested > 0 {
                *width = requested.clamp(1, 120);
            }
        }
    }
    Some(widths)
}

fn measured_text_width(text: &str) -> usize {
    if printable_ascii(text) {
        text.len()
    } else {
        terminal_width(terminal_safe_text(text).as_ref())
    }
}

pub(super) fn table_resize_handles(
    canvas: Rect,
    horizontal_scroll: usize,
    block_areas: &[EditorBlockArea],
    model: &EditorViewModel,
) -> Vec<EditorTableResizeHandle> {
    let mut handles = Vec::new();
    for block_area in block_areas {
        let Some(block) = model.render_blocks().get(block_area.block_index) else {
            continue;
        };
        let (table_id, header, rows) = match block {
            EditorRenderBlock::Table { header, rows, .. } => (None, header, rows),
            EditorRenderBlock::RichTable {
                table_id,
                header,
                rows,
                ..
            } => (Some(*table_id), header, rows),
            _ => continue,
        };
        let widths = table_column_widths(
            header,
            rows,
            model.table_widths_for_block(block_area.block_index, table_id),
        );
        let mut boundary_column = 0usize;
        for (column_index, width) in widths.into_iter().enumerate() {
            boundary_column = boundary_column.saturating_add(width.saturating_add(3));
            let Some(visible_column) = boundary_column.checked_sub(horizontal_scroll) else {
                continue;
            };
            if visible_column >= usize::from(canvas.width) {
                continue;
            }
            handles.push(EditorTableResizeHandle {
                table_id,
                block_index: block_area.block_index,
                column_index,
                width,
                area: Rect::new(
                    canvas.x.saturating_add(to_u16(visible_column)),
                    block_area.area.y,
                    1,
                    block_area.area.height,
                ),
            });
        }
    }
    handles
}

pub(super) fn table_edge_handles(
    canvas: Rect,
    horizontal_scroll: usize,
    block_areas: &[EditorBlockArea],
    model: &EditorViewModel,
) -> Vec<EditorTableEdgeHandle> {
    let mut handles = Vec::new();
    for block_area in block_areas {
        let Some(block) = model.render_blocks().get(block_area.block_index) else {
            continue;
        };
        let (table_id, header, rows) = match block {
            EditorRenderBlock::Table { header, rows, .. } => (None, header, rows),
            EditorRenderBlock::RichTable {
                table_id,
                header,
                rows,
                ..
            } => (Some(*table_id), header, rows),
            _ => continue,
        };
        let source_range = model
            .block_sources
            .get(block_area.block_index)
            .map(|mapping| mapping.source_range);
        if table_id.is_none() && source_range.is_none() {
            continue;
        }
        let widths = table_column_widths(
            header,
            rows,
            model.table_widths_for_block(block_area.block_index, table_id),
        );
        if widths.is_empty() {
            continue;
        }

        if horizontal_scroll == 0 && canvas.width > 0 {
            handles.push(EditorTableEdgeHandle {
                table_id,
                block_index: block_area.block_index,
                edge: EditorTableEdge::Left,
                source_range,
                area: Rect::new(canvas.x, block_area.area.y, 1, block_area.area.height),
            });
        }

        let right_column = widths
            .iter()
            .fold(0usize, |total, width| total.saturating_add(width + 3));
        if let Some(visible_column) = right_column.checked_sub(horizontal_scroll)
            && visible_column < usize::from(canvas.width)
        {
            handles.push(EditorTableEdgeHandle {
                table_id,
                block_index: block_area.block_index,
                edge: EditorTableEdge::Right,
                source_range,
                area: Rect::new(
                    canvas.x.saturating_add(to_u16(visible_column)),
                    block_area.area.y,
                    1,
                    block_area.area.height,
                ),
            });
        }
    }
    handles
}

fn wrap_runs(
    prefix: Vec<DisplayRun>,
    spans: &[EditorRenderSpan],
    width: usize,
    block_index: usize,
    no_wrap: bool,
    source: Option<&str>,
    cell_size: Option<[u16; 2]>,
) -> Vec<DisplayLine> {
    let rich_anchor = spans
        .iter()
        .find_map(|span| span.rich_range.map(|range| range.start));
    let mut prefix = prefix;
    for run in &mut prefix {
        if matches!(run.rich, DisplayRich::Unmapped)
            && let Some(position) = rich_anchor
        {
            run.rich = DisplayRich::Virtual(position);
        }
    }
    let prefix_width = runs_width(&prefix);
    let continuation_anchor = prefix.iter().find_map(display_run_anchor);
    let continuation_prefix = if prefix_width == 0 {
        Vec::new()
    } else {
        vec![
            DisplayRun::virtual_text(
                " ".repeat(prefix_width),
                EditorRenderSpan::plain(""),
                continuation_anchor,
            )
            .with_virtual_rich(rich_anchor),
        ]
    };
    let mut lines = Vec::new();
    let mut current = prefix.clone();
    let mut current_width = prefix_width;
    for span in spans {
        let runs = if let Some(graphic) = span.media.as_ref()
            && let Some(size) =
                inline_graphic_size(span, width.saturating_sub(prefix_width), cell_size)
        {
            vec![DisplayRun::graphic(graphic, span.clone(), size, false)]
        } else {
            mapped_text_runs(&span.text, span.clone(), span.source_range, source)
        };
        for run in runs {
            if run.fixed_width().is_none() && is_display_newline(run.text.resolve(source)) {
                let before = display_run_start(&run);
                let rich_before = display_run_rich_start(&run);
                if before.is_some() || rich_before.is_some() {
                    current.push(
                        DisplayRun::virtual_text("", EditorRenderSpan::plain(""), before)
                            .with_editable_rich_boundary(rich_before),
                    );
                }
                append_wrapped_line(
                    &mut lines,
                    std::mem::take(&mut current),
                    block_index,
                    no_wrap,
                    0,
                );
                current = continuation_prefix.clone();
                let after = display_run_end(&run);
                let rich_after = display_run_rich_end(&run);
                if after.is_some() || rich_after.is_some() {
                    current.push(
                        DisplayRun::virtual_text("", EditorRenderSpan::plain(""), after)
                            .with_editable_rich_boundary(rich_after),
                    );
                }
                current_width = prefix_width;
                continue;
            }
            let run_width = display_run_width(&run);
            if current_width > prefix_width && current_width.saturating_add(run_width) > width {
                append_wrapped_line(
                    &mut lines,
                    std::mem::take(&mut current),
                    block_index,
                    no_wrap,
                    0,
                );
                current = continuation_prefix.clone();
                current_width = prefix_width;
            }
            current_width = current_width.saturating_add(run_width);
            current.push(run);
        }
    }
    if !current.is_empty() || lines.is_empty() {
        append_wrapped_line(&mut lines, current, block_index, no_wrap, 0);
    }
    lines
}

fn append_wrapped_line(
    lines: &mut Vec<DisplayLine>,
    runs: Vec<DisplayRun>,
    block_index: usize,
    no_wrap: bool,
    column_start: usize,
) {
    let height = runs
        .iter()
        .filter_map(|run| match &run.text {
            DisplayText::Graphic(graphic) => Some(graphic.size.height),
            _ => None,
        })
        .max()
        .unwrap_or(1);
    let line = |runs| DisplayLine {
        runs,
        block_index: Some(block_index),
        no_wrap,
        column_start,
        role: DisplayLineRole::Normal,
    };
    let first = lines.len();
    lines.push(line(runs));
    for row in 1..height {
        let mut continuation = Vec::new();
        let mut padding = 0usize;
        for run in &lines[first].runs {
            if let DisplayText::Graphic(graphic) = &run.text
                && row < graphic.size.height
            {
                if padding > 0 {
                    continuation.push(DisplayRun::unmapped(
                        " ".repeat(padding),
                        EditorRenderSpan::default(),
                    ));
                    padding = 0;
                }
                let mut run = run.clone();
                if let DisplayText::Graphic(graphic) = &mut run.text {
                    graphic.row = row;
                }
                continuation.push(run);
            } else {
                padding = padding.saturating_add(display_run_width(run));
            }
        }
        if padding > 0 {
            continuation.push(DisplayRun::unmapped(
                " ".repeat(padding),
                EditorRenderSpan::default(),
            ));
        }
        lines.push(line(continuation));
    }
}

pub(super) fn cell_text(cell: &EditorTableCell) -> String {
    cell.spans
        .iter()
        .map(|span| span.text.as_str())
        .collect::<String>()
}

pub(super) fn fit_text(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    if terminal_width(text) <= width {
        return text.to_string();
    }
    if width == 1 {
        return truncate_to_terminal_width(text, width);
    }
    let mut fitted = truncate_to_terminal_width(text, width.saturating_sub(1));
    fitted.push('…');
    fitted
}

pub(super) fn runs_width(runs: &[DisplayRun]) -> usize {
    runs.iter().map(display_run_width).sum()
}

pub(super) fn display_run_width(run: &DisplayRun) -> usize {
    run.fixed_width()
        .unwrap_or_else(|| measured_text_width(run.text.resolve(None)))
}

pub(super) fn rich_horizontal_content_width(lines: &[DisplayLine], source: Option<&str>) -> usize {
    lines
        .iter()
        .filter(|line| line.no_wrap)
        .map(|line| {
            line.runs
                .iter()
                .map(|run| {
                    run.fixed_width()
                        .unwrap_or_else(|| measured_text_width(run.text.resolve(source)))
                })
                .sum()
        })
        .max()
        .unwrap_or(1)
        .max(1)
}

pub(super) fn display_run_anchor(run: &DisplayRun) -> Option<usize> {
    match run.source {
        DisplaySource::Unmapped => None,
        DisplaySource::Range(range) => Some(range.start),
        DisplaySource::Virtual(offset) => Some(offset),
    }
}

pub(super) fn display_run_start(run: &DisplayRun) -> Option<usize> {
    display_source_start(run.source)
}

pub(super) fn display_run_end(run: &DisplayRun) -> Option<usize> {
    display_source_end(run.source)
}

pub(super) fn display_source_start(mapping: DisplaySource) -> Option<usize> {
    match mapping {
        DisplaySource::Unmapped => None,
        DisplaySource::Range(range) => Some(range.start),
        DisplaySource::Virtual(offset) => Some(offset),
    }
}

pub(super) fn display_source_end(mapping: DisplaySource) -> Option<usize> {
    match mapping {
        DisplaySource::Unmapped => None,
        DisplaySource::Range(range) => Some(range.end),
        DisplaySource::Virtual(offset) => Some(offset),
    }
}

pub(super) fn is_display_newline(value: &str) -> bool {
    matches!(value, "\n" | "\r" | "\r\n")
}

pub(super) fn mapped_text_runs(
    text: &str,
    mut style: EditorRenderSpan,
    source_range: Option<EditorSourceRange>,
    source: Option<&str>,
) -> Vec<DisplayRun> {
    // Each run keeps its own text. Cloning the full span per grapheme is quadratic.
    style.text = String::new();
    style.media = None;
    let exact = source_range.filter(|range| {
        range.start <= range.end
            && source.and_then(|source| source.get(range.start..range.end)) == Some(text)
    });
    let mut runs = Vec::new();
    let mut segment_start = 0usize;
    let mut grapheme_offset = 0usize;
    let context = MappedTextContext {
        style: &style,
        exact,
        source_range,
    };
    let bytes = text.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        let newline_len = match bytes[index] {
            b'\r' if bytes.get(index + 1) == Some(&b'\n') => 2,
            b'\r' | b'\n' => 1,
            _ => {
                index += 1;
                continue;
            }
        };
        append_mapped_grapheme_runs(
            &mut runs,
            &text[segment_start..index],
            segment_start,
            &context,
            &mut grapheme_offset,
        );
        push_mapped_text_run(
            &mut runs,
            &text[index..index + newline_len],
            index..index + newline_len,
            &context,
            grapheme_offset..grapheme_offset.saturating_add(1),
        );
        grapheme_offset = grapheme_offset.saturating_add(1);
        index += newline_len;
        segment_start = index;
    }
    append_mapped_grapheme_runs(
        &mut runs,
        &text[segment_start..],
        segment_start,
        &context,
        &mut grapheme_offset,
    );
    if runs.is_empty() {
        let rich = empty_rich_mapping(style.rich_range);
        runs.push(DisplayRun {
            text: DisplayText::Owned(String::new()),
            style,
            source: exact
                .map(DisplaySource::Range)
                .or_else(|| source_range.map(|range| DisplaySource::Virtual(range.start)))
                .unwrap_or(DisplaySource::Unmapped),
            rich,
        });
    }
    runs
}

pub(super) struct MappedTextContext<'a> {
    style: &'a EditorRenderSpan,
    exact: Option<EditorSourceRange>,
    source_range: Option<EditorSourceRange>,
}

pub(super) fn append_mapped_grapheme_runs(
    runs: &mut Vec<DisplayRun>,
    text: &str,
    base: usize,
    context: &MappedTextContext<'_>,
    grapheme_offset: &mut usize,
) {
    let span = Span::raw(text.to_owned());
    let mut relative = 0usize;
    for grapheme in span.styled_graphemes(Style::default()) {
        let start = base.saturating_add(relative);
        relative = relative.saturating_add(grapheme.symbol.len());
        push_mapped_text_run(
            runs,
            grapheme.symbol,
            start..base.saturating_add(relative),
            context,
            *grapheme_offset..grapheme_offset.saturating_add(1),
        );
        *grapheme_offset = grapheme_offset.saturating_add(1);
    }
}

pub(super) fn push_mapped_text_run(
    runs: &mut Vec<DisplayRun>,
    text: &str,
    source_offsets: std::ops::Range<usize>,
    context: &MappedTextContext<'_>,
    grapheme_offsets: std::ops::Range<usize>,
) {
    let source = match (context.exact, context.source_range) {
        (Some(range), _) => DisplaySource::Range(EditorSourceRange::new(
            range
                .start
                .saturating_add(source_offsets.start)
                .min(range.end),
            range
                .start
                .saturating_add(source_offsets.end)
                .min(range.end),
        )),
        // Decoded entities, generated labels and other non-exact text must
        // never pretend that every visible grapheme covers the whole source.
        (None, Some(range)) => DisplaySource::Virtual(range.start),
        (None, None) => DisplaySource::Unmapped,
    };
    let range = context.style.rich_range;
    let atomic = range.filter(|range| {
        range.start.container_id == range.end.container_id
            && range.end.grapheme_offset == range.start.grapheme_offset.saturating_add(1)
    });
    runs.push(DisplayRun {
        text: DisplayText::Owned(text.to_owned()),
        style: context.style.clone(),
        source,
        rich: atomic.map_or_else(
            || rich_mapping(range, grapheme_offsets.start, grapheme_offsets.end),
            DisplayRich::Range,
        ),
    });
}

pub(super) fn empty_rich_mapping(range: Option<RichRange>) -> DisplayRich {
    match range {
        Some(range) if range.start.container_id == range.end.container_id => {
            DisplayRich::Range(range)
        }
        Some(range) => DisplayRich::Virtual(range.start),
        None => DisplayRich::Unmapped,
    }
}

pub(super) fn rich_mapping(
    range: Option<RichRange>,
    relative_start: usize,
    relative_end: usize,
) -> DisplayRich {
    let Some(range) = range else {
        return DisplayRich::Unmapped;
    };
    if range.start.container_id != range.end.container_id {
        return DisplayRich::Virtual(range.start);
    }
    let start = range
        .start
        .grapheme_offset
        .saturating_add(relative_start)
        .min(range.end.grapheme_offset);
    let end = range
        .start
        .grapheme_offset
        .saturating_add(relative_end)
        .min(range.end.grapheme_offset);
    DisplayRich::Range(RichRange::between(
        RichPosition::in_node(range.start.container_id, start),
        RichPosition::in_node(range.start.container_id, end),
    ))
}

pub(super) fn display_source_for_segment(
    mapping: DisplaySource,
    text_len: usize,
    relative_start: usize,
    relative_end: usize,
) -> DisplaySource {
    match mapping {
        DisplaySource::Range(range)
            if range.end.saturating_sub(range.start) == text_len
                && relative_start <= relative_end =>
        {
            DisplaySource::Range(EditorSourceRange::new(
                range.start.saturating_add(relative_start).min(range.end),
                range.start.saturating_add(relative_end).min(range.end),
            ))
        }
        mapping => mapping,
    }
}

pub(super) fn display_rich_for_grapheme(
    mapping: DisplayRich,
    relative_start: usize,
    relative_end: usize,
) -> DisplayRich {
    match mapping {
        DisplayRich::Range(range)
            if range.start.container_id == range.end.container_id
                && range
                    .end
                    .grapheme_offset
                    .saturating_sub(range.start.grapheme_offset)
                    >= relative_end =>
        {
            DisplayRich::Range(RichRange::between(
                RichPosition::in_node(
                    range.start.container_id,
                    range.start.grapheme_offset.saturating_add(relative_start),
                ),
                RichPosition::in_node(
                    range.start.container_id,
                    range.start.grapheme_offset.saturating_add(relative_end),
                ),
            ))
        }
        mapping => mapping,
    }
}

pub(super) fn display_line_source_boundaries(
    line: &DisplayLine,
    source: Option<&str>,
    horizontal_start: usize,
    width: usize,
) -> Vec<EditorSourceBoundary> {
    let mut boundaries = Vec::new();
    let mut fallback_boundary = None;
    let mut measure = DisplayColumnMeasure::new(line);
    let horizontal_end = horizontal_start.saturating_add(width);
    for run in &line.runs {
        if let Some(run_width) = run.fixed_width() {
            let column = measure.column();
            let next = measure.push(run_width);
            let editable = matches!(run.source, DisplaySource::Range(_));
            if let Some(offset) = display_run_end(run) {
                fallback_boundary = Some(EditorSourceBoundary {
                    column: next,
                    byte_offset: offset,
                    editable,
                });
            }
            if next >= horizontal_start && column <= horizontal_end {
                if let Some(offset) = display_run_start(run) {
                    push_source_boundary(&mut boundaries, column, offset, editable);
                }
                if let Some(offset) = display_run_end(run) {
                    push_source_boundary(&mut boundaries, next, offset, editable);
                }
            }
            if next > horizontal_end {
                break;
            }
            continue;
        }
        let text = run.text.resolve(source);
        if text.is_empty() {
            let column = measure.column();
            if let Some(offset) = display_run_start(run) {
                push_source_boundary(
                    &mut boundaries,
                    column,
                    offset,
                    matches!(run.source, DisplaySource::Range(_)),
                );
            }
            if let Some(offset) = display_run_end(run) {
                push_source_boundary(
                    &mut boundaries,
                    column,
                    offset,
                    matches!(run.source, DisplaySource::Range(_)),
                );
            }
            continue;
        }
        let span = Span::raw(text);
        let mut relative_byte = 0usize;
        for grapheme in span.styled_graphemes(Style::default()) {
            let relative_start = relative_byte;
            relative_byte = relative_byte.saturating_add(grapheme.symbol.len());
            let mapping =
                display_source_for_segment(run.source, text.len(), relative_start, relative_byte);
            let safe = terminal_safe_text(grapheme.symbol);
            let column = measure.column();
            let next_column = measure.push(terminal_width(safe.as_ref()).max(1));
            let intersects = next_column >= horizontal_start && column <= horizontal_end;
            if let Some(offset) = display_source_end(mapping) {
                fallback_boundary = Some(EditorSourceBoundary {
                    column: next_column,
                    byte_offset: offset,
                    editable: matches!(mapping, DisplaySource::Range(_)),
                });
            }
            if intersects && let Some(offset) = display_source_start(mapping) {
                push_source_boundary(
                    &mut boundaries,
                    column,
                    offset,
                    matches!(mapping, DisplaySource::Range(_)),
                );
            }
            if intersects && let Some(offset) = display_source_end(mapping) {
                push_source_boundary(
                    &mut boundaries,
                    next_column,
                    offset,
                    matches!(mapping, DisplaySource::Range(_)),
                );
            }
            if next_column > horizontal_end {
                break;
            }
        }
        if measure.column() > horizontal_end {
            break;
        }
    }
    if boundaries.is_empty()
        && let Some(boundary) = fallback_boundary
    {
        boundaries.push(boundary);
    }
    boundaries
}

pub(super) fn display_line_rich_boundaries(
    line: &DisplayLine,
    source: Option<&str>,
    horizontal_start: usize,
    width: usize,
) -> Vec<EditorRichBoundary> {
    let mut boundaries = Vec::new();
    let mut fallback_boundary = None;
    let mut measure = DisplayColumnMeasure::new(line);
    let horizontal_end = horizontal_start.saturating_add(width);
    for run in &line.runs {
        if let Some(run_width) = run.fixed_width() {
            let column = measure.column();
            let next = measure.push(run_width);
            let editable = matches!(run.rich, DisplayRich::Range(_));
            if let Some(position) = display_run_rich_end(run) {
                fallback_boundary = Some(EditorRichBoundary {
                    column: next,
                    position,
                    editable,
                });
            }
            if next >= horizontal_start && column <= horizontal_end {
                if let Some(position) = display_run_rich_start(run) {
                    push_rich_boundary(&mut boundaries, column, position, editable);
                }
                if let Some(position) = display_run_rich_end(run) {
                    push_rich_boundary(&mut boundaries, next, position, editable);
                }
            }
            if next > horizontal_end {
                break;
            }
            continue;
        }
        let text = run.text.resolve(source);
        if text.is_empty() {
            let column = measure.column();
            if let Some(position) = display_run_rich_start(run) {
                push_rich_boundary(
                    &mut boundaries,
                    column,
                    position,
                    matches!(run.rich, DisplayRich::Range(_)),
                );
            }
            if let Some(position) = display_run_rich_end(run) {
                push_rich_boundary(
                    &mut boundaries,
                    column,
                    position,
                    matches!(run.rich, DisplayRich::Range(_)),
                );
            }
            continue;
        }
        let span = Span::raw(text);
        let mut relative_grapheme = 0usize;
        for grapheme in span.styled_graphemes(Style::default()) {
            let mapping = display_rich_for_grapheme(
                run.rich,
                relative_grapheme,
                relative_grapheme.saturating_add(1),
            );
            relative_grapheme = relative_grapheme.saturating_add(1);
            let safe = terminal_safe_text(grapheme.symbol);
            let column = measure.column();
            let next_column = measure.push(terminal_width(safe.as_ref()).max(1));
            let intersects = next_column >= horizontal_start && column <= horizontal_end;
            if let Some(position) = display_rich_end(mapping) {
                fallback_boundary = Some(EditorRichBoundary {
                    column: next_column,
                    position,
                    editable: matches!(mapping, DisplayRich::Range(_)),
                });
            }
            if intersects && let Some(position) = display_rich_start(mapping) {
                push_rich_boundary(
                    &mut boundaries,
                    column,
                    position,
                    matches!(mapping, DisplayRich::Range(_)),
                );
            }
            if intersects && let Some(position) = display_rich_end(mapping) {
                push_rich_boundary(
                    &mut boundaries,
                    next_column,
                    position,
                    matches!(mapping, DisplayRich::Range(_)),
                );
            }
            if next_column > horizontal_end {
                break;
            }
        }
        if measure.column() > horizontal_end {
            break;
        }
    }
    if boundaries.is_empty()
        && let Some(boundary) = fallback_boundary
    {
        boundaries.push(boundary);
    }
    boundaries
}

pub(super) fn display_run_rich_start(run: &DisplayRun) -> Option<RichPosition> {
    display_rich_start(run.rich)
}

pub(super) fn display_rich_start(mapping: DisplayRich) -> Option<RichPosition> {
    match mapping {
        DisplayRich::Unmapped => None,
        DisplayRich::Range(range) => Some(range.start),
        DisplayRich::Virtual(position) => Some(position),
    }
}

pub(super) fn display_run_rich_end(run: &DisplayRun) -> Option<RichPosition> {
    display_rich_end(run.rich)
}

pub(super) fn display_rich_end(mapping: DisplayRich) -> Option<RichPosition> {
    match mapping {
        DisplayRich::Unmapped => None,
        DisplayRich::Range(range) => Some(range.end),
        DisplayRich::Virtual(position) => Some(position),
    }
}

pub(super) fn push_rich_boundary(
    boundaries: &mut Vec<EditorRichBoundary>,
    column: usize,
    position: RichPosition,
    editable: bool,
) {
    if let Some(last) = boundaries
        .last_mut()
        .filter(|last| last.column == column && last.position == position)
    {
        last.editable |= editable;
        return;
    }
    boundaries.push(EditorRichBoundary {
        column,
        position,
        editable,
    });
}

pub(super) fn push_source_boundary(
    boundaries: &mut Vec<EditorSourceBoundary>,
    column: usize,
    byte_offset: usize,
    editable: bool,
) {
    if let Some(last) = boundaries
        .last_mut()
        .filter(|last| last.column == column && last.byte_offset == byte_offset)
    {
        last.editable |= editable;
        return;
    }
    boundaries.push(EditorSourceBoundary {
        column,
        byte_offset,
        editable,
    });
}

pub(super) fn nearest_source_boundary(
    boundaries: &[EditorSourceBoundary],
    column: usize,
) -> Option<EditorSourceBoundary> {
    boundaries.iter().copied().min_by_key(|boundary| {
        (
            boundary.column.abs_diff(column),
            usize::from(!boundary.editable),
            usize::from(boundary.column < column),
        )
    })
}

pub(super) fn nearest_rich_boundary(
    boundaries: &[EditorRichBoundary],
    column: usize,
) -> Option<EditorRichBoundary> {
    boundaries.iter().copied().min_by_key(|boundary| {
        (
            boundary.column.abs_diff(column),
            usize::from(!boundary.editable),
            usize::from(boundary.column < column),
        )
    })
}

pub(super) fn nearest_editable_rich_boundary(
    lines: &[EditorRichLineMap],
    visual: EditorTextPosition,
    position: RichPosition,
) -> Option<EditorRichBoundary> {
    lines
        .iter()
        .flat_map(|line| {
            line.boundaries
                .iter()
                .filter(move |boundary| {
                    boundary.editable && boundary.position.container_id == position.container_id
                })
                .map(move |boundary| {
                    (
                        (
                            boundary
                                .position
                                .grapheme_offset
                                .abs_diff(position.grapheme_offset),
                            line.document_line.abs_diff(visual.line),
                            boundary.column.abs_diff(visual.column),
                        ),
                        *boundary,
                    )
                })
        })
        .min_by_key(|(distance, _)| *distance)
        .map(|(_, boundary)| boundary)
}

pub(super) fn nearest_visual_position(
    lines: &[EditorSourceLineMap],
    byte_offset: usize,
) -> Option<EditorTextPosition> {
    lines
        .iter()
        .flat_map(|line| {
            line.boundaries.iter().map(move |boundary| {
                (
                    (
                        boundary.byte_offset.abs_diff(byte_offset),
                        usize::from(!boundary.editable),
                    ),
                    EditorTextPosition::new(line.document_line, boundary.column),
                )
            })
        })
        .min_by_key(|(distance, position)| (*distance, position.line, position.column))
        .map(|(_, position)| position)
}

pub(super) fn nearest_visual_position_for_rich(
    lines: &[EditorRichLineMap],
    position: RichPosition,
) -> Option<EditorTextPosition> {
    lines
        .iter()
        .flat_map(|line| {
            line.boundaries
                .iter()
                .filter(move |boundary| boundary.position.container_id == position.container_id)
                .map(move |boundary| {
                    (
                        (
                            boundary
                                .position
                                .grapheme_offset
                                .abs_diff(position.grapheme_offset),
                            usize::from(!boundary.editable),
                        ),
                        EditorTextPosition::new(line.document_line, boundary.column),
                    )
                })
        })
        .min_by_key(|(distance, visual)| (*distance, visual.line, visual.column))
        .map(|(_, visual)| visual)
}

pub(super) fn code_line_source_ranges(
    lines: &[String],
    block_source: Option<EditorBlockSourceMap>,
    source: Option<&str>,
) -> Vec<Option<EditorSourceRange>> {
    let Some(content) = block_source.and_then(|mapping| mapping.content_range) else {
        return vec![None; lines.len()];
    };
    let Some(source) = source else {
        return vec![None; lines.len()];
    };
    let Some(content_source) = source.get(content.start..content.end) else {
        return vec![None; lines.len()];
    };
    let mut cursor = 0usize;
    let mut ranges = Vec::with_capacity(lines.len());
    for line in lines {
        if !content_source[cursor.min(content_source.len())..].starts_with(line) {
            return vec![None; lines.len()];
        }
        let start = content.start.saturating_add(cursor);
        cursor = cursor.saturating_add(line.len()).min(content_source.len());
        ranges.push(Some(EditorSourceRange::new(
            start,
            content.start.saturating_add(cursor),
        )));
        let remaining = &content_source[cursor..];
        if remaining.starts_with("\r\n") {
            cursor += 2;
        } else if remaining.starts_with(['\r', '\n']) {
            cursor += 1;
        }
    }
    ranges
}

/// Replaces bytes that a terminal could interpret as control traffic with
/// visible, inert Unicode glyphs. This must be applied at the final rendering
/// boundary as documents, file names, and status messages are all untrusted.
///
/// C0 controls use the Unicode Control Pictures block, DEL uses its dedicated
/// symbol, and C1/bidirectional formatting controls use the replacement glyph.
/// Newlines are laid out structurally before this boundary; if one reaches this
/// function it is shown visibly instead of being emitted to the terminal.
pub(super) fn accent_span() -> EditorRenderSpan {
    EditorRenderSpan {
        color: EditorSpanColor::Accent,
        bold: true,
        ..EditorRenderSpan::default()
    }
}

pub(super) fn muted_span() -> EditorRenderSpan {
    EditorRenderSpan {
        color: EditorSpanColor::Muted,
        ..EditorRenderSpan::default()
    }
}

pub(super) fn warning_span() -> EditorRenderSpan {
    EditorRenderSpan {
        color: EditorSpanColor::Warning,
        ..EditorRenderSpan::default()
    }
}

pub(super) fn empty_display_line(block_index: Option<usize>) -> DisplayLine {
    empty_display_line_at(block_index, None)
}

pub(super) fn empty_display_line_at(
    block_index: Option<usize>,
    byte_offset: Option<usize>,
) -> DisplayLine {
    DisplayLine {
        runs: vec![DisplayRun::virtual_text(
            "",
            EditorRenderSpan::default(),
            byte_offset,
        )],
        block_index,
        no_wrap: false,
        column_start: 0,
        role: DisplayLineRole::Normal,
    }
}
