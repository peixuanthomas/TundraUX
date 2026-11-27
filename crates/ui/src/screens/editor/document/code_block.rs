use super::*;

pub(super) fn code_block_lines(
    language: Option<&str>,
    block_index: usize,
    anchor: Option<usize>,
    end_anchor: Option<usize>,
    mut lines: impl Iterator<Item = Vec<DisplayRun>>,
) -> Vec<DisplayLine> {
    let line = |runs| DisplayLine {
        runs,
        block_index: Some(block_index),
        no_wrap: true,
        column_start: 0,
        role: DisplayLineRole::Normal,
    };
    let first = lines.next();
    let rich_start = first
        .as_ref()
        .and_then(|runs| runs.iter().find_map(display_run_rich_start));
    let mut output = vec![line(vec![
        DisplayRun::virtual_text(code_block_heading(language), accent_span(), anchor)
            .with_virtual_rich(rich_start),
    ])];
    let mut rich_end = rich_start;
    output.extend(first.into_iter().chain(lines).map(|mut runs| {
        let line_anchor = runs.iter().find_map(display_run_start).or(anchor);
        let rich_start = runs.iter().find_map(display_run_rich_start).or(rich_end);
        rich_end = runs
            .iter()
            .rev()
            .find_map(display_run_rich_end)
            .or(rich_start);
        runs.insert(
            0,
            DisplayRun::virtual_text("│ ", accent_span(), line_anchor)
                .with_virtual_rich(rich_start),
        );
        line(runs)
    }));
    output.push(line(vec![
        DisplayRun::virtual_text("└─", accent_span(), end_anchor).with_virtual_rich(rich_end),
    ]));
    output
}

pub(super) fn code_block_heading(language: Option<&str>) -> String {
    match language {
        Some(language) => format!("┌─ {language} "),
        None => format!("┌─{}", i18n::tr!("ui-editor-code-padded")),
    }
}

pub(super) fn rich_code_block_lines(
    language: Option<&str>,
    block_index: usize,
    lines: &[EditorRenderSpan],
    anchor: Option<usize>,
    source: Option<&str>,
    visible: Range<usize>,
) -> Vec<DisplayLine> {
    let start = visible.start.saturating_sub(1).min(lines.len());
    let end = visible.end.saturating_sub(1).min(lines.len());
    let runs = lines[start..end]
        .iter()
        .map(|span| rich_code_line_runs(span, source));
    let mut output = code_block_lines(language, block_index, anchor, anchor, runs);
    if visible.start > 0 {
        output.remove(0);
    } else if let Some(range) = lines.first().and_then(|span| span.rich_range) {
        output[0].runs[0].rich = DisplayRich::Virtual(range.start);
    }
    if visible.end <= lines.len().saturating_add(1) {
        output.pop();
    } else if let Some(range) = lines.last().and_then(|span| span.rich_range) {
        output.last_mut().unwrap().runs[0].rich = DisplayRich::Virtual(range.end);
    }
    output
}

pub(super) fn code_tab_width(column: usize) -> usize {
    4 - column % 4
}

pub(super) fn code_tab_segments(text: &str) -> impl Iterator<Item = (&str, bool)> {
    text.split_inclusive('\t').map(|part| {
        part.strip_suffix('\t')
            .map_or((part, false), |text| (text, true))
    })
}

fn rich_code_line_runs(span: &EditorRenderSpan, source: Option<&str>) -> Vec<DisplayRun> {
    let mut style = span.clone();
    style.inline_code = false;
    if !span.text.contains('\t') {
        return mapped_text_runs(&span.text, style, span.source_range, source);
    }
    style.text.clear();
    let context = MappedTextContext {
        style: &style,
        exact: span.source_range.filter(|range| {
            source.and_then(|source| source.get(range.start..range.end)) == Some(&span.text)
        }),
        source_range: span.source_range,
    };
    let (mut at, mut grapheme, mut column) = (0, 0, 0usize);
    let mut runs = Vec::new();
    for (text, ends_with_tab) in code_tab_segments(&span.text) {
        let first = runs.len();
        append_mapped_grapheme_runs(&mut runs, text, at, &context, &mut grapheme);
        at += text.len();
        column = column.saturating_add(runs_width(&runs[first..]));
        if ends_with_tab {
            let tab = code_tab_width(column);
            push_mapped_text_run(
                &mut runs,
                "\t",
                at..at + 1,
                &context,
                grapheme..grapheme + 1,
            );
            runs.last_mut().unwrap().text = DisplayText::CodeTab(tab);
            column += tab;
            grapheme += 1;
            at += 1;
        }
    }
    runs
}
