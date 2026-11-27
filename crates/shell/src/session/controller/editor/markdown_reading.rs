use std::sync::atomic::{AtomicBool, Ordering};

use app::{
    editor::{PreparedMarkdownReading, reading_media::ReadingMediaStyle},
    rich_document::RichProjection,
};

use super::preview_layout::EditorReadOnlyGeometry;
use super::{editor_reading_render_blocks, editor_reading_text_blocks};
use crate::session::*;

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReadingKey {
    document_generation: u64,
    revision: u64,
    path: PathBuf,
    language_generation: u64,
    style: Option<ReadingMediaStyle>,
    native_heading: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ReadingCache {
    key: ReadingKey,
    prepared: Option<PreparedMarkdownReading>,
    media_projection: Option<Arc<RichProjection>>,
    pub(super) blocks: Arc<[ui::EditorRenderBlock]>,
}

struct MeasuredReading {
    cache: ReadingCache,
    layout: ui::EditorReadOnlyLayout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReadingStage {
    Text,
    Media,
}

impl MeasuredReading {
    fn prepare(
        cache: ReadingCache,
        geometry: EditorReadOnlyGeometry,
        cancelled: impl Fn() -> bool,
    ) -> Option<Self> {
        let layout = geometry.measure(Arc::clone(&cache.blocks), cancelled)?;
        Some(Self { cache, layout })
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(in crate::session) struct EditorMarkdownReadingJob {
    request: Option<(ReadingKey, ReadingStage)>,
    cache: Option<ReadingCache>,
    text_worker: app::background::Latest<Option<MeasuredReading>>,
    media_worker: app::background::Latest<Option<MeasuredReading>>,
}

impl EditorMarkdownReadingJob {
    fn worker(&self, stage: ReadingStage) -> &app::background::Latest<Option<MeasuredReading>> {
        match stage {
            ReadingStage::Text => &self.text_worker,
            ReadingStage::Media => &self.media_worker,
        }
    }

    fn cancel(&mut self) {
        self.request = None;
        self.text_worker.cancel();
        self.media_worker.cancel();
    }

    pub(super) fn clear(&mut self) {
        self.cancel();
        self.cache = None;
    }

    pub(in crate::session) fn pending(&self) -> bool {
        self.request.is_some()
    }
}

impl ShellSession {
    pub(super) fn editor_markdown_reading_toggle(&self) -> Option<bool> {
        if self.pages.editor.script.enabled
            || self.config_editor_active()
            || self.pages.editor.editor_load_state.is_some()
            || self.editor_vim_terminal.is_some()
            || self.editor_vim_enabled()
        {
            return None;
        }
        self.app
            .editor_state()
            .filter(|editor| editor.can_toggle_markdown_reading())
            .map(EditorState::is_markdown_reading)
    }

    fn editor_markdown_reading_key(&self) -> Option<ReadingKey> {
        let editor = self.app.editor_state()?;
        Some(ReadingKey {
            document_generation: self.pages.editor.editor_document_generation,
            revision: editor.revision(),
            path: editor.document.path.clone()?,
            language_generation: self.language.generation(),
            style: self.editor_reading_media_style(),
            native_heading: self.terminal_text_sizing_support,
        })
    }

    pub(super) fn editor_markdown_reading_cache(&self) -> Option<&ReadingCache> {
        self.pages
            .editor
            .editor_markdown_reading
            .cache
            .as_ref()
            .filter(|cache| {
                cache.prepared.is_none()
                    && self.editor_markdown_reading_key().as_ref() == Some(&cache.key)
            })
    }

    pub(in crate::session) fn editor_markdown_reading_needs_preparation(&self) -> bool {
        self.app
            .editor_state()
            .is_some_and(EditorState::is_markdown_reading)
            && self.editor_markdown_reading_cache().is_none()
    }

    pub(super) fn toggle_editor_markdown_reading(&mut self) {
        let Some(reading) = self.editor_markdown_reading_toggle() else {
            return;
        };
        if self.pages.editor.editor_search_bar.is_some() {
            self.activate_edit_search_control(ui::EditorSearchControl::Close);
        }
        self.clear_editor_scrollbar_drag();
        self.pages.editor.editor_cursor_acceleration = None;
        self.pages.editor.editor_drag_anchor = None;
        self.pages.editor.editor_quick_menu_anchor = None;
        self.pages.editor.editor_table_resize = None;
        if reading {
            self.stop_editor_markdown_reading();
        } else {
            self.start_editor_markdown_reading();
        }
        self.refresh_hit_map();
    }

    fn start_editor_markdown_reading(&mut self) -> bool {
        let Some(key) = self.editor_markdown_reading_key() else {
            return false;
        };
        let Some(snapshot) = self.app.begin_editor_markdown_reading() else {
            return false;
        };
        let geometry = self.editor_read_only_geometry(None);
        let cache = self
            .pages
            .editor
            .editor_markdown_reading
            .cache
            .take()
            .filter(|cache| cache.key == key && cache.prepared.is_some());
        if let Some(cache) = cache {
            if self
                .pages
                .editor
                .editor_read_only_layout
                .ready_for(geometry.key)
            {
                return self.publish_editor_markdown_reading(cache);
            }
            return self.submit_editor_markdown_reading(
                key,
                ReadingStage::Text,
                move |cancelled| {
                    MeasuredReading::prepare(cache, geometry, || cancelled.load(Ordering::Relaxed))
                },
            );
        }

        self.pages.editor.editor_read_only_layout.clear();
        let language = self.language.clone();
        self.submit_editor_markdown_reading(key.clone(), ReadingStage::Text, move |cancelled| {
            let cancelled = || cancelled.load(Ordering::Relaxed);
            if cancelled() {
                return None;
            }
            let (prepared, projection) = snapshot.parse().ok()?;
            let _language = i18n::enter_snapshot(language);
            let (blocks, has_media) = editor_reading_text_blocks(
                Some(&key.path),
                &[],
                &projection,
                key.style,
                key.native_heading,
                cancelled,
            );
            MeasuredReading::prepare(
                ReadingCache {
                    key,
                    prepared: Some(prepared),
                    media_projection: has_media.then(|| Arc::new(projection)),
                    blocks: blocks.into(),
                },
                geometry,
                cancelled,
            )
        })
    }

    fn submit_editor_markdown_reading(
        &mut self,
        key: ReadingKey,
        stage: ReadingStage,
        work: impl FnOnce(&AtomicBool) -> Option<MeasuredReading> + Send + 'static,
    ) -> bool {
        let job = &mut self.ui.pages.editor.editor_markdown_reading;
        let submitted = self
            .ui
            .editor_task_runtime
            .shared
            .task_group
            .as_ref()
            .is_some_and(|group| job.worker(stage).submit_in(group, work).is_ok());
        job.request = submitted.then_some((key, stage));
        if !submitted {
            self.fail_editor_markdown_reading();
        }
        submitted
    }

    fn publish_editor_markdown_reading(&mut self, mut cache: ReadingCache) -> bool {
        if let Some(prepared) = cache.prepared.take()
            && !self.app.install_editor_markdown_reading(prepared)
        {
            self.fail_editor_markdown_reading();
            return false;
        }
        let media = cache.media_projection.clone();
        let key = cache.key.clone();
        self.pages.editor.editor_markdown_reading.cache = Some(cache);
        if let Some(projection) = media {
            let geometry = self.editor_read_only_geometry(None);
            let language = self.language.clone();
            self.submit_editor_markdown_reading(
                key.clone(),
                ReadingStage::Media,
                move |cancelled| {
                    let cancelled = || cancelled.load(Ordering::Relaxed);
                    let _language = i18n::enter_snapshot(language);
                    let blocks = editor_reading_render_blocks(
                        Some(&key.path),
                        &[],
                        &projection,
                        key.style,
                        key.native_heading,
                        cancelled,
                    );
                    MeasuredReading::prepare(
                        ReadingCache {
                            key,
                            prepared: None,
                            media_projection: None,
                            blocks: blocks.into(),
                        },
                        geometry,
                        cancelled,
                    )
                },
            );
        }
        true
    }

    fn stop_editor_markdown_reading(&mut self) {
        let prepared = self.app.end_editor_markdown_reading();
        let job = &mut self.pages.editor.editor_markdown_reading;
        job.cancel();
        if let Some(cache) = &mut job.cache {
            cache.prepared = prepared;
        }
        self.pages.editor.editor_read_only_layout.cancel_pending();
    }

    fn fail_editor_markdown_reading(&mut self) {
        let job = &mut self.pages.editor.editor_markdown_reading;
        if let Some(cache) = &mut job.cache {
            cache.media_projection = None;
            job.cancel();
            return;
        }
        self.stop_editor_markdown_reading();
        self.pages.editor.editor_message =
            Some(i18n::msg!("shell-markdown-reading-unavailable").into());
    }

    pub(super) fn poll_editor_markdown_reading(&mut self) -> bool {
        let Some(editor) = self.app.editor_state().filter(|s| s.is_markdown_reading()) else {
            let job = &mut self.pages.editor.editor_markdown_reading;
            if job.pending() {
                job.cancel();
            }
            return false;
        };
        let viewport = editor.viewport;
        let key = self.editor_markdown_reading_key();
        let job = &mut self.ui.pages.editor.editor_markdown_reading;
        let current = job
            .request
            .as_ref()
            .map(|(key, _)| key)
            .or_else(|| job.cache.as_ref().map(|cache| &cache.key));
        if current.is_none() || current != key.as_ref() {
            // A Save As or capability change can invalidate prepared graphics while reading.
            self.stop_editor_markdown_reading();
            if self.start_editor_markdown_reading() {
                self.app
                    .dispatch_at(app::AppCommand::SetEditorViewport(viewport), Instant::now());
            }
            return true;
        }
        let Some((_, stage)) = job.request.as_ref() else {
            return false;
        };
        let stage = *stage;
        match job.worker(stage).take() {
            Some(Some(update)) if Some(&update.cache.key) == key.as_ref() => {
                job.request = None;
                let geometry = self.editor_read_only_geometry(None);
                if self.pages.editor.selected_id() == Some(self.content_page_id())
                    && update.layout.key() != geometry.key
                {
                    let key = update.cache.key.clone();
                    return !self.submit_editor_markdown_reading(key, stage, move |cancelled| {
                        MeasuredReading::prepare(update.cache, geometry, || {
                            cancelled.load(Ordering::Relaxed)
                        })
                    });
                }
                self.pages
                    .editor
                    .editor_read_only_layout
                    .install_prepared(update.layout);
                self.publish_editor_markdown_reading(update.cache);
                true
            }
            None if job.worker(stage).busy() => false,
            _ => {
                self.fail_editor_markdown_reading();
                true
            }
        }
    }
}

#[cfg(test)]
#[path = "../../../../tests/unit/session/controller/editor/markdown_reading_tests.rs"]
mod tests;
