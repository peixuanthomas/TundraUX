use std::sync::atomic::Ordering;

use super::editor_reading_render_blocks;
use crate::session::*;

pub(in crate::session) type EditorOfficeGraphics = app::background::Latest<Option<OfficeView>>;

pub(in crate::session) struct OfficeView {
    blocks: Arc<[ui::EditorRenderBlock]>,
    layout: ui::EditorReadOnlyLayout,
}

impl ShellSession {
    pub(super) fn prepare_editor_office_graphics(
        &mut self,
        projection: app::rich_document::RichProjection,
    ) {
        let Some(editor) = self.app.editor_state() else {
            return;
        };
        let Some(style) = self.editor_reading_media_style() else {
            return;
        };
        let geometry = self.editor_read_only_geometry(editor.rich_cursor());
        let native_heading = self.terminal_text_sizing_support;
        let images = editor.embedded_preview_images().to_vec();
        let Some(group) = self.editor_task_runtime.shared.task_group.as_ref() else {
            return;
        };
        let language = self.language.clone();
        let _ = self
            .pages
            .editor
            .editor_office_graphics
            .submit_in(group, move |cancel| {
                let cancelled = || cancel.load(Ordering::Relaxed);
                let _language = i18n::enter_snapshot(language);
                let blocks = editor_reading_render_blocks(
                    None,
                    &images,
                    &projection,
                    Some(style),
                    native_heading,
                    cancelled,
                )
                .into();
                let layout = geometry.measure(Arc::clone(&blocks), cancelled)?;
                Some(OfficeView { blocks, layout })
            });
    }

    pub(super) fn poll_editor_office_graphics(&mut self) -> bool {
        let Some(Some(view)) = self.pages.editor.editor_office_graphics.take() else {
            return false;
        };
        let Some(editor) = self.app.editor_state() else {
            return false;
        };
        let geometry = self.editor_read_only_geometry(editor.rich_cursor());
        if geometry.key != view.layout.key() {
            if let Some(group) = self.editor_task_runtime.shared.task_group.as_ref() {
                let _ = self
                    .pages
                    .editor
                    .editor_office_graphics
                    .submit_in(group, move |cancel| {
                        let blocks = view.blocks;
                        let layout = geometry
                            .measure(Arc::clone(&blocks), || cancel.load(Ordering::Relaxed))?;
                        Some(OfficeView { blocks, layout })
                    });
            }
            return false;
        }
        let Some(cache) = self.pages.editor.editor_rich_render_cache.as_mut() else {
            return false;
        };
        cache.blocks = view.blocks;
        self.pages
            .editor
            .editor_read_only_layout
            .install_prepared(view.layout);
        true
    }
}
