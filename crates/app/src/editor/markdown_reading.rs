use super::*;
use super::{markdown_codec::MarkdownCodecError, rich_edit::RichReadingNavigation};

pub fn is_markdown_path(path: &Path) -> bool {
    path.extension().is_some_and(|extension| {
        ["md", "markdown", "mdown", "mkd"]
            .iter()
            .any(|&suffix| extension.eq_ignore_ascii_case(suffix))
    })
}

/// An immutable Rope snapshot that can be parsed off the UI thread.
pub struct MarkdownReadingSnapshot {
    text: Rope,
    metadata: TextMetadata,
    revision: u64,
}

impl MarkdownReadingSnapshot {
    pub fn parse(self) -> Result<(PreparedMarkdownReading, RichProjection), MarkdownCodecError> {
        let document = MarkdownCodec::import_for_reading(
            &Cow::from(&self.text),
            self.metadata.utf8_bom,
            rich_line_ending(self.metadata.preferred_line_ending),
        )?
        .document;
        let projection = document.project();
        let word_count = rich_edit::projected_plain_text(&projection)
            .unicode_words()
            .count();
        Ok((
            PreparedMarkdownReading {
                first_position: document.first_editable_position(),
                document: Arc::new(document),
                navigation: Arc::new(RichReadingNavigation::new(&projection)),
                word_count,
                revision: self.revision,
            },
            projection,
        ))
    }
}

/// Parsed content that can move between the editor and the host's view cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedMarkdownReading {
    document: Arc<RichDocument>,
    navigation: Arc<RichReadingNavigation>,
    first_position: Option<RichPosition>,
    word_count: usize,
    revision: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MarkdownReading {
    pub(super) cursor: Cursor,
    pub(super) selection: Option<Selection>,
    viewport: EditorViewport,
    ready: Option<Box<ReadyMarkdownReading>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReadyMarkdownReading {
    source: SourceBuffer,
    first_position: Option<RichPosition>,
    word_count: usize,
}

impl EditorState {
    pub fn can_toggle_markdown_reading(&self) -> bool {
        self.is_markdown_reading()
            || (self.can_save()
                && self.mode == EditorMode::Source
                && self.document.kind == DocumentKind::PlainText
                && self.document.path.as_deref().is_some_and(is_markdown_path))
    }

    /// True throughout loading and while the rendered view is active.
    pub const fn is_markdown_reading(&self) -> bool {
        self.markdown_reading.is_some()
    }

    pub(super) const fn markdown_reading_pending(&self) -> bool {
        self.is_markdown_reading() && matches!(self.mode, EditorMode::Source)
    }

    pub fn markdown_reading_source_viewport(&self) -> Option<EditorViewport> {
        self.markdown_reading
            .as_ref()
            .filter(|reading| reading.ready.is_none())
            .map(|reading| reading.viewport)
    }

    pub fn begin_markdown_reading(&mut self) -> Option<MarkdownReadingSnapshot> {
        if self.is_markdown_reading() || !self.can_toggle_markdown_reading() {
            return None;
        }
        let text = self.source_rope_snapshot()?;
        self.markdown_reading = Some(MarkdownReading {
            cursor: self.cursor,
            selection: self.selection,
            viewport: self.viewport,
            ready: None,
        });
        self.viewport = EditorViewport::default();
        self.search_job.cancel();
        if self.search.as_ref().is_some_and(|search| search.pending) {
            self.search = None;
        }
        Some(MarkdownReadingSnapshot {
            text,
            metadata: self.document.metadata,
            revision: self.revision,
        })
    }

    pub fn install_markdown_reading(&mut self, preview: PreparedMarkdownReading) -> bool {
        let Some(reading) = &mut self.markdown_reading else {
            return false;
        };
        if reading.ready.is_some() || preview.revision != self.revision {
            return false;
        }
        let mut editor = RichEditor::with_cursor(
            Arc::unwrap_or_clone(preview.document),
            preview.first_position,
        );
        editor.reading_navigation = Some(preview.navigation);
        let original =
            std::mem::replace(&mut self.buffer, EditorBuffer::Rich(RichBuffer { editor }));
        let EditorBuffer::Source(source) = original else {
            unreachable!("Markdown reading starts from Source mode");
        };
        reading.ready = Some(Box::new(ReadyMarkdownReading {
            source,
            first_position: preview.first_position,
            word_count: preview.word_count,
        }));
        self.mode = EditorMode::Rich;
        true
    }

    /// Restores Source immediately, even when the parse hasn't finished.
    pub fn end_markdown_reading(&mut self) -> Option<PreparedMarkdownReading> {
        let reading = self.markdown_reading.take()?;
        self.cursor = reading.cursor;
        self.selection = reading.selection;
        self.viewport = reading.viewport;
        let ready = reading.ready?;
        let original = std::mem::replace(&mut self.buffer, EditorBuffer::Source(ready.source));
        let EditorBuffer::Rich(buffer) = original else {
            unreachable!("Markdown reading with retained Source displays Rich mode");
        };
        self.mode = EditorMode::Source;
        Some(PreparedMarkdownReading {
            document: Arc::new(buffer.editor.document),
            navigation: buffer
                .editor
                .reading_navigation
                .expect("installed Markdown reading has a navigation index"),
            first_position: ready.first_position,
            word_count: ready.word_count,
            revision: self.revision,
        })
    }

    pub(super) fn canonical_source_buffer(&self) -> Option<&SourceBuffer> {
        match &self.buffer {
            EditorBuffer::Source(buffer) => Some(buffer),
            EditorBuffer::Rich(_) => Some(&self.markdown_reading.as_ref()?.ready.as_ref()?.source),
        }
    }

    pub(super) fn markdown_reading_word_count(&self) -> Option<usize> {
        Some(self.markdown_reading.as_ref()?.ready.as_ref()?.word_count)
    }
}
