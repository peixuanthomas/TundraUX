use std::ops::Range;

use super::*;
use crate::screens::editor::picture::{self, PictureBounds};

#[cfg(test)]
std::thread_local! {
    pub(in crate::screens::editor) static READ_ONLY_MEASURE_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[derive(Debug, PartialEq, Eq)]
pub(in crate::screens::editor) struct ReadOnlyRichLayout {
    width: usize,
    picture_bounds: PictureBounds,
    block_ends: Vec<usize>,
    tables: BTreeMap<usize, TableMetrics>,
    pub(in crate::screens::editor) content_width: usize,
}

impl ReadOnlyRichLayout {
    #[cfg(test)]
    pub(in crate::screens::editor) fn measure(
        model: &EditorViewModel,
        width: usize,
        height: usize,
    ) -> Self {
        Self::measure_cancellable(model, width, height, &|| false).unwrap()
    }

    pub(in crate::screens::editor) fn measure_cancellable(
        model: &EditorViewModel,
        width: usize,
        height: usize,
        cancelled: &impl Fn() -> bool,
    ) -> Option<Self> {
        #[cfg(test)]
        READ_ONLY_MEASURE_CALLS.with(|count| count.set(count.get() + 1));
        let picture_bounds = picture::bounds(width, height, model.image_cell_size);
        let mut block_ends = Vec::with_capacity(model.render_blocks().len());
        let mut tables = BTreeMap::new();
        let mut end = 0usize;
        let mut content_width = 1;
        for (index, block) in model.render_blocks().iter().enumerate() {
            if cancelled() {
                return None;
            }
            let lines = match block {
                EditorRenderBlock::Table { header, rows, .. }
                | EditorRenderBlock::RichTable { header, rows, .. } => {
                    let table = TableMetrics::measure(
                        header,
                        rows,
                        model.table_widths_for_block(index, block.table_id()),
                        media_cell_size(model),
                        cancelled,
                    )?;
                    content_width = content_width.max(table.content_width());
                    let lines = table.line_count;
                    tables.insert(index, table);
                    lines
                }
                _ => {
                    content_width = content_width.max(unwrapped_width(block, width));
                    rich_block_line_count_up_to_with_heading(
                        block,
                        width,
                        usize::MAX,
                        None,
                        BlockPresentation::for_block(model, block, picture_bounds),
                    )
                    .unwrap()
                }
            };
            end = end.saturating_add(lines);
            block_ends.push(end);
        }
        Some(Self {
            width,
            picture_bounds,
            block_ends,
            tables,
            content_width,
        })
    }

    pub(in crate::screens::editor) fn line_count(&self) -> usize {
        self.block_ends.last().copied().unwrap_or(1)
    }

    pub(in crate::screens::editor) fn materialize(
        &self,
        model: &EditorViewModel,
        visible: Range<usize>,
    ) -> Vec<DisplayLine> {
        if visible.is_empty() {
            return Vec::new();
        }
        if self.block_ends.is_empty() {
            return vec![empty_display_line(None)];
        }
        let mut lines = Vec::with_capacity(visible.len());
        let first = self.block_ends.partition_point(|&end| end <= visible.start);
        for index in first..self.block_ends.len() {
            let start = if index == 0 {
                0
            } else {
                self.block_ends[index - 1]
            };
            if start >= visible.end {
                break;
            }
            let within = intersect_lines(&visible, start, self.block_ends[index]);
            if let Some(metrics) = self.tables.get(&index) {
                lines.extend(materialize_rich_block(
                    model,
                    index,
                    self.width,
                    self.picture_bounds,
                    Some(TableWindow {
                        metrics,
                        lines: within,
                    }),
                ));
            } else if let EditorRenderBlock::RichCodeBlock {
                language,
                lines: code,
            } = &model.render_blocks()[index]
            {
                let anchor = model
                    .block_sources
                    .get(index)
                    .copied()
                    .map(EditorBlockSourceMap::anchor);
                lines.extend(rich_code_block_lines(
                    language.as_deref(),
                    index,
                    code,
                    anchor,
                    model.source.as_deref(),
                    within,
                ));
            } else {
                lines.extend(
                    materialize_rich_block(model, index, self.width, self.picture_bounds, None)
                        .into_iter()
                        .skip(within.start)
                        .take(within.len()),
                );
            }
        }
        debug_assert_eq!(lines.len(), visible.len());
        lines
    }
}

pub(super) struct TableWindow<'a> {
    pub(super) metrics: &'a TableMetrics,
    pub(super) lines: Range<usize>,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct TableMetrics {
    pub(super) widths: Vec<usize>,
    pub(super) header_end: usize,
    pub(super) row_ends: Vec<usize>,
    pub(super) line_count: usize,
}

impl TableMetrics {
    fn measure(
        header: &[EditorTableCell],
        rows: &[Vec<EditorTableCell>],
        requested: Option<&[usize]>,
        cell_size: Option<[u16; 2]>,
        cancelled: &impl Fn() -> bool,
    ) -> Option<Self> {
        let widths =
            table_column_widths_cancellable(header, rows, requested, cell_size, cancelled)?;
        let has_header = !header.is_empty();
        let row_height =
            |row| wrapped_table_row_height(row, &widths, usize::MAX, cell_size).unwrap();
        let header_end: usize = if has_header {
            1 + row_height(header)
        } else {
            1
        };
        let mut end = header_end.saturating_add(usize::from(has_header));
        let row_ends = if widths.is_empty() {
            Vec::new()
        } else {
            rows.iter()
                .map(|row| {
                    if cancelled() {
                        return None;
                    }
                    end = end.saturating_add(row_height(row));
                    Some(end)
                })
                .collect::<Option<Vec<_>>>()?
        };
        let line_count = if widths.is_empty() {
            1
        } else {
            end.saturating_add(1)
        };
        Some(Self {
            widths,
            header_end,
            row_ends,
            line_count,
        })
    }

    fn content_width(&self) -> usize {
        self.widths
            .iter()
            .fold(1, |total, width| total.saturating_add(width + 3))
    }
}

pub(super) fn intersect_lines(visible: &Range<usize>, start: usize, end: usize) -> Range<usize> {
    visible.start.saturating_sub(start)..visible.end.min(end).saturating_sub(start)
}

fn unwrapped_width(block: &EditorRenderBlock, width: usize) -> usize {
    match block {
        EditorRenderBlock::CodeBlock { language, lines } => code_width(
            language.as_deref(),
            lines.iter().map(|line| mapped_text_width(line)),
        ),
        EditorRenderBlock::RichCodeBlock { language, lines } => {
            let widths = lines.iter().map(|line| {
                if !line.text.contains('\t') {
                    return mapped_text_width(&line.text);
                }
                code_tab_segments(&line.text).fold(0usize, |column, (text, ends_with_tab)| {
                    let end = column.saturating_add(mapped_text_width(text));
                    end.saturating_add(if ends_with_tab {
                        code_tab_width(end)
                    } else {
                        0
                    })
                })
            });
            code_width(language.as_deref(), widths)
        }
        EditorRenderBlock::HorizontalRule => width,
        EditorRenderBlock::RawHtml(html) => source_display_line_ranges(html)
            .into_iter()
            .enumerate()
            .map(|(index, range)| {
                mapped_text_width(&html[range.start..range.end]).saturating_add(if index == 0 {
                    5
                } else {
                    0
                })
            })
            .max()
            .unwrap(),
        EditorRenderBlock::Image { markdown } => mapped_text_width(markdown),
        _ => 1,
    }
}

fn code_width(language: Option<&str>, widths: impl Iterator<Item = usize>) -> usize {
    let top = code_block_heading(language);
    widths
        .max()
        .unwrap_or(0)
        .saturating_add(2)
        .max(mapped_text_width(&top))
}

fn mapped_text_width(text: &str) -> usize {
    if printable_ascii(text) {
        return text.len();
    }
    let line_breaks = text
        .bytes()
        .filter(|byte| matches!(byte, b'\r' | b'\n'))
        .count();
    Span::raw(text)
        .styled_graphemes(Style::default())
        .map(|grapheme| terminal_width(terminal_safe_text(grapheme.symbol).as_ref()))
        .fold(line_breaks, usize::saturating_add)
}
