use std::sync::atomic::Ordering;

use ui::{EditorReadOnlyLayout, EditorReadOnlyLayoutKey, EditorReadOnlyLayoutState};

use crate::session::*;

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
    fn editor_read_only_layout_key(&self) -> Option<EditorReadOnlyLayoutKey> {
        if self.content_screen() != ShellScreen::Editor
            || self.pages.editor.editor_load_state.is_some()
        {
            return None;
        }
        let state = self.app.editor_state()?;
        let cache = self.pages.editor.editor_rich_render_cache.as_ref()?;
        if state.access() != app::editor::EditorAccess::ReadOnly
            || state.mode != app::editor::EditorMode::Rich
            || cache.revision != state.revision()
            || !cache.is_large(state)
        {
            return None;
        }
        let area = self.editor_content_rect();
        if area.is_empty() {
            return None;
        }
        let _language = i18n::enter_snapshot(self.language.clone());
        Some(EditorReadOnlyLayoutKey::for_area(
            area,
            self.pages.editor.editor_search_bar.as_ref(),
            self.terminal_image_cell_size,
            self.terminal_text_sizing_support,
            state.rich_cursor(),
        ))
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
                job.state = Some(EditorReadOnlyLayoutState::Ready(Arc::new(layout)));
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
