use std::borrow::Cow;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use app::editor::reading_media::{self, ReadingMedia, ReadingMediaStyle};
use app::editor::{EditorState, PreviewImage};
use app::rich_document::{
    NodeId, ProjectedBlock, ProjectedBlockKind, ProjectedInline, ProjectedTableCell, RichListKind,
    RichPosition, RichProjection, RichRange, RichTableAlignment,
};
use ui::{
    EditorPreviewGraphic, EditorPreviewGraphicKind, EditorRenderBlock, EditorRenderSpan,
    EditorSpanColor, EditorTableAlignment, EditorTableCell,
};
use unicode_segmentation::UnicodeSegmentation;

const MAX_PREPARED_MEDIA_BYTES: usize = 32 * 1024 * 1024;
const MAX_PREPARED_MEDIA: usize = 512;
const MAX_ASSET_BYTES: usize = 4 * 1024 * 1024;

pub(in crate::session) fn editor_rich_render_blocks(
    state: &EditorState,
    projection: &RichProjection,
) -> Vec<EditorRenderBlock> {
    RichBlocks {
        reading: state.is_read_only(),
        embedded_images: state.embedded_preview_images(),
        media: ReadingMediaWork::Disabled,
        cancelled: &|| false,
        output: Vec::new(),
    }
    .build(projection)
    .0
}

pub(in crate::session) fn editor_reading_text_blocks(
    document_path: Option<&Path>,
    office_images: &[PreviewImage],
    projection: &RichProjection,
    style: Option<ReadingMediaStyle>,
    native_heading: bool,
    cancelled: impl Fn() -> bool,
) -> (Vec<EditorRenderBlock>, bool) {
    RichBlocks {
        reading: true,
        embedded_images: office_images,
        media: ReadingMediaWork::Detect {
            local_image: document_path.is_some(),
            native_image: style.is_some(),
            native_heading,
            found: false,
        },
        cancelled: &cancelled,
        output: Vec::new(),
    }
    .build(projection)
}

pub(in crate::session) fn editor_reading_render_blocks(
    document_path: Option<&Path>,
    office_images: &[PreviewImage],
    projection: &RichProjection,
    style: Option<ReadingMediaStyle>,
    native_heading: bool,
    cancelled: impl Fn() -> bool,
) -> Vec<EditorRenderBlock> {
    RichBlocks {
        reading: true,
        embedded_images: office_images,
        media: ReadingMediaWork::Prepare(ReadingGraphics {
            document_path,
            style,
            native_heading,
            remaining_bytes: MAX_PREPARED_MEDIA_BYTES,
            remaining_renders: MAX_PREPARED_MEDIA,
            prepared: HashMap::new(),
        }),
        cancelled: &cancelled,
        output: Vec::new(),
    }
    .build(projection)
    .0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum GraphicKind {
    Image,
    Math { display: bool },
    Mermaid,
    Heading { level: u8 },
}

impl GraphicKind {
    fn to_source(self, source: &str) -> ReadingMedia<'_> {
        match self {
            Self::Image => ReadingMedia::Image {
                destination: source,
            },
            Self::Math { display } => ReadingMedia::Math { source, display },
            Self::Mermaid => ReadingMedia::Mermaid { source },
            Self::Heading { level } => ReadingMedia::Heading {
                text: source,
                level,
            },
        }
    }

    fn supported(self, local_image: bool, native_image: bool, native_heading: bool) -> bool {
        match self {
            Self::Image => local_image,
            Self::Heading { .. } => native_image && !native_heading,
            Self::Math { .. } | Self::Mermaid => native_image,
        }
    }
}

enum ReadingMediaWork<'a, 'source> {
    Disabled,
    Detect {
        local_image: bool,
        native_image: bool,
        native_heading: bool,
        found: bool,
    },
    Prepare(ReadingGraphics<'a, 'source>),
}

impl ReadingMediaWork<'_, '_> {
    fn wants(&self, kind: GraphicKind) -> bool {
        match self {
            Self::Disabled => false,
            Self::Detect {
                local_image,
                native_image,
                native_heading,
                found,
            } => !found && kind.supported(*local_image, *native_image, *native_heading),
            Self::Prepare(graphics) => kind.supported(
                graphics.document_path.is_some(),
                graphics.style.is_some(),
                graphics.native_heading,
            ),
        }
    }
}

type MediaBySource<'source> = HashMap<Cow<'source, str>, Option<EditorPreviewGraphic>>;

struct ReadingGraphics<'a, 'source> {
    document_path: Option<&'a Path>,
    style: Option<ReadingMediaStyle>,
    native_heading: bool,
    remaining_bytes: usize,
    remaining_renders: usize,
    prepared: HashMap<GraphicKind, MediaBySource<'source>>,
}

impl<'source> ReadingGraphics<'_, 'source> {
    fn prepare(
        &mut self,
        kind: GraphicKind,
        source: Cow<'source, str>,
    ) -> Option<EditorPreviewGraphic> {
        if let Some(cached) = self
            .prepared
            .get(&kind)
            .and_then(|cache| cache.get(source.as_ref()))
        {
            return cached.clone();
        }
        let style = match kind {
            GraphicKind::Image
                if self.document_path.is_some()
                    && reading_media::is_local_image_reference(&source) =>
            {
                self.style.unwrap_or(ReadingMediaStyle {
                    foreground: [0; 3],
                    background: [0; 3],
                    cell_size: [1, 2],
                })
            }
            GraphicKind::Image => return None,
            _ => self.style?,
        };
        if self.remaining_renders == 0 || self.remaining_bytes < 8 {
            return None;
        }
        self.remaining_renders -= 1;
        let budget = self.remaining_bytes.min(MAX_ASSET_BYTES);
        let prepared =
            reading_media::render(kind.to_source(&source), self.document_path, style, budget).map(
                |image| {
                    self.remaining_bytes = self.remaining_bytes.saturating_sub(image.byte_len());
                    EditorPreviewGraphic {
                        id: super::tasks::next_editor_task_id(),
                        image: Arc::new(image),
                    }
                },
            );
        self.prepared
            .entry(kind)
            .or_default()
            .insert(source, prepared.clone());
        prepared
    }
}

struct RichBlocks<'a, 'source> {
    reading: bool,
    embedded_images: &'a [PreviewImage],
    media: ReadingMediaWork<'a, 'source>,
    cancelled: &'a dyn Fn() -> bool,
    output: Vec<EditorRenderBlock>,
}

impl<'source> RichBlocks<'_, 'source> {
    fn build(mut self, projection: &'source RichProjection) -> (Vec<EditorRenderBlock>, bool) {
        for block in &projection.blocks {
            self.append_block(block, 0);
            if self.cancelled() {
                break;
            }
        }
        if self.output.is_empty() {
            self.output.push(EditorRenderBlock::Blank);
        }
        (
            self.output,
            matches!(self.media, ReadingMediaWork::Detect { found: true, .. }),
        )
    }

    fn cancelled(&self) -> bool {
        (self.cancelled)()
    }

    fn graphic(
        &mut self,
        kind: GraphicKind,
        source: impl Into<Cow<'source, str>>,
    ) -> Option<EditorPreviewGraphic> {
        if !self.media.wants(kind) || self.cancelled() {
            return None;
        }
        let source = source.into();
        match &mut self.media {
            ReadingMediaWork::Disabled => None,
            ReadingMediaWork::Detect { found, .. } => {
                *found = if kind == GraphicKind::Image {
                    reading_media::is_local_image_reference(&source)
                } else {
                    !source.trim().is_empty()
                };
                None
            }
            ReadingMediaWork::Prepare(graphics) => graphics.prepare(kind, source),
        }
    }

    fn image_block(&mut self, span: &'source ProjectedInline) -> Option<EditorRenderBlock> {
        let destination = &span.image.as_ref()?.url;
        if let Some(image) = PreviewImage::embedded_from_url(self.embedded_images, destination) {
            return Some(EditorRenderBlock::Picture {
                id: super::tasks::next_editor_task_id(),
                image: image.clone(),
            });
        }
        let graphic = self.graphic(GraphicKind::Image, destination.as_str())?;
        Some(preview_graphic(
            graphic,
            EditorPreviewGraphicKind::Picture,
            &span.text,
            span.range,
        ))
    }

    fn append_block(&mut self, block: &'source ProjectedBlock, depth: u8) {
        if self.cancelled() {
            return;
        }
        match &block.kind {
            ProjectedBlockKind::Paragraph { content } => {
                self.append_text(block.id, content, EditorRenderBlock::Paragraph);
            }
            ProjectedBlockKind::Heading { level, content } => {
                let level = *level;
                let kind = GraphicKind::Heading { level };
                if self.media.wants(kind)
                    && content
                        .iter()
                        .all(|span| span.image.is_none() && span.math.is_none())
                {
                    let text: String = content.iter().map(|span| span.text.as_str()).collect();
                    if let Some(graphic) = self.graphic(kind, text) {
                        let spans = self.text_spans(block.id, content);
                        self.output.push(EditorRenderBlock::PreviewHeading {
                            level,
                            spans,
                            graphic,
                        });
                        return;
                    }
                }
                self.append_text(block.id, content, |spans| EditorRenderBlock::Heading {
                    level,
                    spans,
                });
            }
            ProjectedBlockKind::Quote { blocks } => {
                let depth = depth.saturating_add(1);
                for nested in blocks {
                    match &nested.kind {
                        ProjectedBlockKind::Paragraph { content }
                        | ProjectedBlockKind::Heading { content, .. } => {
                            self.append_text(nested.id, content, |spans| {
                                EditorRenderBlock::Quote { depth, spans }
                            });
                        }
                        _ => self.append_block(nested, depth),
                    }
                }
            }
            ProjectedBlockKind::CodeBlock {
                language,
                code,
                range,
            } => {
                if !self.reading {
                    let mut span = EditorRenderSpan::code(code).with_rich_range(*range);
                    span.color = EditorSpanColor::Muted;
                    self.output.push(EditorRenderBlock::Paragraph(vec![span]));
                    return;
                }
                let media = match language.as_deref() {
                    Some(language) if language.eq_ignore_ascii_case("math") => Some((
                        GraphicKind::Math { display: true },
                        EditorPreviewGraphicKind::Math,
                    )),
                    Some(language) if language.eq_ignore_ascii_case("mermaid") => {
                        Some((GraphicKind::Mermaid, EditorPreviewGraphicKind::Mermaid))
                    }
                    _ => None,
                };
                let rendered = media
                    .and_then(|(media, kind)| {
                        self.graphic(media, code.as_str())
                            .map(|graphic| preview_graphic(graphic, kind, code, *range))
                    })
                    .unwrap_or_else(|| code_block(language.clone(), code, *range));
                self.output.push(rendered);
            }
            ProjectedBlockKind::List {
                kind, start, items, ..
            } => {
                for (index, item) in items.iter().enumerate() {
                    let mut primary = None;
                    let mut nested = Vec::new();
                    for item_block in &item.blocks {
                        match &item_block.kind {
                            ProjectedBlockKind::Paragraph { content }
                            | ProjectedBlockKind::Heading { content, .. }
                                if primary.is_none() =>
                            {
                                primary = Some((item_block.id, content.as_slice()));
                            }
                            _ => nested.push(item_block),
                        }
                    }
                    let (id, content) = primary.unwrap_or((item.id, &[]));
                    self.append_text(id, content, |spans| match kind {
                        RichListKind::Bullet | RichListKind::Task => {
                            EditorRenderBlock::BulletListItem {
                                depth,
                                checked: if *kind == RichListKind::Task {
                                    item.checked.or(Some(false))
                                } else {
                                    None
                                },
                                spans,
                            }
                        }
                        RichListKind::Ordered => EditorRenderBlock::OrderedListItem {
                            depth,
                            number: start.saturating_add(index) as u64,
                            spans,
                        },
                    });
                    for nested_block in nested {
                        self.append_block(nested_block, depth.saturating_add(1));
                    }
                }
            }
            ProjectedBlockKind::Table {
                alignments,
                header,
                rows,
            } => {
                let table = EditorRenderBlock::RichTable {
                    table_id: block.id,
                    header: header.iter().map(|cell| self.table_cell(cell)).collect(),
                    rows: rows
                        .iter()
                        .map(|row| row.iter().map(|cell| self.table_cell(cell)).collect())
                        .collect(),
                    alignments: alignments
                        .iter()
                        .map(|alignment| match alignment {
                            RichTableAlignment::None | RichTableAlignment::Left => {
                                EditorTableAlignment::Left
                            }
                            RichTableAlignment::Center => EditorTableAlignment::Center,
                            RichTableAlignment::Right => EditorTableAlignment::Right,
                        })
                        .collect(),
                };
                self.output.push(table);
                for span in header
                    .iter()
                    .chain(rows.iter().flatten())
                    .flat_map(|cell| &cell.content)
                {
                    if let Some(image) = self.image_block(span) {
                        self.output.push(image);
                    }
                }
            }
            ProjectedBlockKind::Rule => self.output.push(EditorRenderBlock::HorizontalRule),
            ProjectedBlockKind::OpaqueMarkdown { raw, reason } => {
                let text = if self.reading {
                    raw.clone()
                } else {
                    i18n::tr!(
                        "shell-unsupported-markdown-read-only-reason-nraw",
                        reason = reason.to_string(),
                        raw = raw
                    )
                };
                self.output.push(EditorRenderBlock::RawHtml(text));
            }
        }
    }

    fn append_text(
        &mut self,
        id: NodeId,
        content: &'source [ProjectedInline],
        text_block: impl Fn(Vec<EditorRenderSpan>) -> EditorRenderBlock,
    ) {
        let mut start = 0;
        for (index, span) in content.iter().enumerate() {
            let block = if span.math == Some(true) {
                self.graphic(GraphicKind::Math { display: true }, span.text.as_str())
                    .map(|graphic| {
                        preview_graphic(
                            graphic,
                            EditorPreviewGraphicKind::Math,
                            &span.text,
                            span.range,
                        )
                    })
                    .or_else(|| Some(text_block(self.text_spans(id, &content[index..=index]))))
            } else {
                self.image_block(span)
            };
            let Some(block) = block else { continue };
            if start < index {
                let spans = self.text_spans(id, &content[start..index]);
                self.output.push(text_block(spans));
            }
            self.output.push(block);
            start = index + 1;
        }
        if start == 0 || start < content.len() {
            let spans = self.text_spans(id, &content[start..]);
            self.output.push(text_block(spans));
        }
    }

    fn table_cell(&mut self, cell: &'source ProjectedTableCell) -> EditorTableCell {
        EditorTableCell {
            spans: self.text_spans(cell.id, &cell.content),
        }
    }

    fn text_spans(
        &mut self,
        id: NodeId,
        spans: &'source [ProjectedInline],
    ) -> Vec<EditorRenderSpan> {
        if spans.is_empty() {
            return vec![EditorRenderSpan::plain("").with_rich_range(RichRange::in_node(id, 0, 0))];
        }
        spans
            .iter()
            .map(|span| {
                let mut rendered = EditorRenderSpan::plain(&span.text).with_rich_range(span.range);
                rendered.bold = span.marks.bold;
                rendered.italic = span.marks.italic || span.math.is_some();
                rendered.strikethrough = span.marks.strikethrough;
                rendered.inline_code = span.marks.code;
                if span.link.is_some() {
                    rendered = rendered.with_link();
                }
                if span.image.is_some() {
                    rendered.color = EditorSpanColor::Accent;
                    rendered.underlined = true;
                }
                if span.math == Some(false) {
                    rendered.media =
                        self.graphic(GraphicKind::Math { display: false }, span.text.as_str());
                }
                rendered
            })
            .collect()
    }
}

fn code_block(language: Option<String>, code: &str, range: RichRange) -> EditorRenderBlock {
    let mut start = range.start;
    let lines = code
        .split_terminator('\n')
        .map(|line| {
            let end = RichPosition::in_node(
                start.container_id,
                start.grapheme_offset + line.graphemes(true).count(),
            );
            let span = EditorRenderSpan::code(line).with_rich_range(RichRange::between(start, end));
            start.grapheme_offset = end.grapheme_offset + 1;
            span
        })
        .collect();
    EditorRenderBlock::RichCodeBlock { language, lines }
}

fn preview_graphic(
    graphic: EditorPreviewGraphic,
    kind: EditorPreviewGraphicKind,
    fallback: &str,
    range: RichRange,
) -> EditorRenderBlock {
    EditorRenderBlock::PreviewGraphic {
        graphic,
        kind,
        fallback: fallback.to_owned(),
        range: Some(range),
    }
}

#[cfg(test)]
#[path = "../../../../tests/unit/session/workflows/rich_blocks.rs"]
mod tests;
