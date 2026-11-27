use std::sync::atomic::Ordering;

use ui::{EditorReadOnlyLayout, EditorReadOnlyLayoutKey, EditorReadOnlyLayoutState};

use crate::session::*;

#[derive(Clone, Copy)]
pub(super) struct EditorReadOnlyGeometry {
    pub(super) key: EditorReadOnlyLayoutKey,
    cell_size: [u16; 2],
    text_sizing: bool,
    image_protocol: ui::EditorImageProtocolStatus,
    rich_cursor: Option<ui::RichPosition>,
}

impl EditorReadOnlyGeometry {
    pub(super) fn measure(
        self,
        blocks: Arc<[ui::EditorRenderBlock]>,
        cancelled: impl Fn() -> bool,
    ) -> Option<EditorReadOnlyLayout> {
        let mut model = ui::EditorViewModel::new_shared("", blocks);
        model.read_only = true;
        model.image_cell_size = self.cell_size;
        model.image_protocol = self.image_protocol;
        model.text_sizing_protocol = self.text_sizing;
        model.rich_cursor = self.rich_cursor;
        EditorReadOnlyLayout::prepare(self.key, &model, cancelled)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(in crate::session) struct EditorReadOnlyLayoutJob {
    key: Option<EditorReadOnlyLayoutKey>,
    state: Option<EditorReadOnlyLayoutState>,
    worker: app::background::Latest<Option<EditorReadOnlyLayout>>,
}

impl EditorReadOnlyLayoutJob {
    pub(super) fn view_state(&self) -> EditorReadOnlyLayoutState {
        self.state
            .clone()
            .unwrap_or(EditorReadOnlyLayoutState::Preparing)
    }

    pub(super) fn cancel_pending(&mut self) {
        if matches!(self.state, Some(EditorReadOnlyLayoutState::Preparing)) {
            self.clear();
        }
    }

    pub(super) fn clear(&mut self) {
        self.worker.cancel();
        self.key = None;
        self.state = None;
    }

    pub(super) fn ready_for(&self, key: EditorReadOnlyLayoutKey) -> bool {
        self.key == Some(key) && matches!(self.state, Some(EditorReadOnlyLayoutState::Ready(_)))
    }

    pub(super) fn install_prepared(&mut self, layout: EditorReadOnlyLayout) {
        self.worker.cancel();
        self.key = Some(layout.key());
        self.state = Some(EditorReadOnlyLayoutState::Ready(Arc::new(layout)));
    }

    fn finished(&self, key: EditorReadOnlyLayoutKey) -> bool {
        self.key == Some(key)
            && matches!(
                self.state,
                Some(
                    EditorReadOnlyLayoutState::Ready(_) | EditorReadOnlyLayoutState::Unavailable(_)
                )
            )
    }
}

impl ShellSession {
    pub(super) fn editor_read_only_geometry(
        &self,
        rich_cursor: Option<ui::RichPosition>,
    ) -> EditorReadOnlyGeometry {
        let cell_size = self.terminal_image_cell_size;
        let text_sizing = self.terminal_text_sizing_support;
        let image_protocol = self.editor_image_protocol();
        let _language = i18n::enter_snapshot(self.language.clone());
        let key = EditorReadOnlyLayoutKey::for_area(
            self.editor_content_rect(),
            self.pages.editor.editor_search_bar.as_ref(),
            cell_size,
            text_sizing,
            rich_cursor,
        )
        .with_image_protocol(image_protocol);
        EditorReadOnlyGeometry {
            key,
            cell_size,
            text_sizing,
            image_protocol,
            rich_cursor,
        }
    }

    fn editor_read_only_layout_key(&self) -> Option<EditorReadOnlyLayoutKey> {
        if self.content_screen() != ShellScreen::Editor
            || self.pages.editor.editor_load_state.is_some()
        {
            return None;
        }
        let state = self.app.editor_state()?;
        let reading = state.is_markdown_reading();
        let needs_measurement = if reading {
            self.editor_markdown_reading_cache().is_some()
        } else {
            let cache = self.pages.editor.editor_rich_render_cache.as_ref()?;
            state.access() == app::editor::EditorAccess::ReadOnly
                && cache.revision == state.revision()
                && cache.is_large(state)
        };
        if state.mode != app::editor::EditorMode::Rich
            || self
                .pages
                .editor
                .editor_picture_zoom
                .is_some_and(ui::EditorPreviewZoom::is_zoomed)
            || !needs_measurement
        {
            return None;
        }
        if self.editor_content_rect().is_empty() {
            return None;
        }
        Some(
            self.editor_read_only_geometry(if reading { None } else { state.rich_cursor() })
                .key,
        )
    }

    pub(in crate::session) fn editor_read_only_layout_has_background_work(&self) -> bool {
        self.editor_read_only_layout_key()
            .is_some_and(|key| !self.pages.editor.editor_read_only_layout.finished(key))
    }

    pub(super) fn poll_editor_read_only_layout(&mut self) -> bool {
        let Some(key) = self.editor_read_only_layout_key() else {
            self.pages.editor.editor_read_only_layout.cancel_pending();
            return false;
        };
        let job = &mut self.pages.editor.editor_read_only_layout;
        if job.finished(key) {
            return false;
        }
        if job.key == Some(key) {
            if let Some(Some(layout)) = job.worker.take()
                && layout.key() == key
            {
                job.install_prepared(layout);
            } else if job.worker.busy() {
                return false;
            } else {
                job.state = Some(EditorReadOnlyLayoutState::Unavailable(key));
            }
            self.refresh_hit_map();
            return true;
        }

        let model = self.to_editor_view_model();
        let session_ui = &mut self.ui;
        let job = &mut session_ui.pages.editor.editor_read_only_layout;
        job.key = Some(key);
        let submitted = session_ui
            .editor_task_runtime
            .shared
            .task_group
            .as_ref()
            .is_some_and(|group| {
                job.worker
                    .submit_in(group, move |cancelled| {
                        EditorReadOnlyLayout::prepare(key, &model, || {
                            cancelled.load(Ordering::Relaxed)
                        })
                    })
                    .is_ok()
            });
        let state = if submitted {
            EditorReadOnlyLayoutState::Preparing
        } else {
            EditorReadOnlyLayoutState::Unavailable(key)
        };
        job.state = Some(state);
        !submitted
    }
}
