use app::editor::{PdfPageImage, PdfPreviewDocument};
use i18n::LocalizedText;
use ui::EditorPdfAction::{self, Next, Open, Previous};

use crate::session::*;

type PageRender = Result<(u64, PdfPageImage), LocalizedText>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::session) struct EditorPdfPreview {
    document: PdfPreviewDocument,
    page: usize,
    focus: EditorPdfAction,
    viewport: Option<[u16; 2]>,
    result: Option<PageRender>,
    task: app::background::Latest<PageRender>,
}

impl EditorPdfPreview {
    pub(super) fn new(document: PdfPreviewDocument) -> Self {
        Self {
            document,
            page: 0,
            focus: Open,
            viewport: None,
            result: None,
            task: Default::default(),
        }
    }

    pub(super) fn view_model(&self) -> ui::EditorPdfViewModel {
        let result = self.result.as_ref();
        ui::EditorPdfViewModel {
            page: self.page,
            total: self.document.page_count(),
            focus: self.focus,
            image: result.and_then(|result| result.as_ref().ok()).cloned(),
            error: result
                .and_then(|result| result.as_ref().err())
                .map(LocalizedText::render_current),
        }
    }

    fn turn(&mut self, action: EditorPdfAction) {
        if !action.enabled(self.page, self.document.page_count()) {
            return;
        }
        self.page = match action {
            Previous => self.page - 1,
            Next => self.page + 1,
            Open => return,
        };
        if !self.focus.enabled(self.page, self.document.page_count()) {
            self.focus = Open;
        }
        self.viewport = None;
        self.result = None;
        self.task.cancel();
    }

    fn move_focus(&mut self, backwards: bool) {
        let actions = EditorPdfAction::ALL;
        let index = actions
            .iter()
            .position(|&action| action == self.focus)
            .unwrap_or(0);
        let offset = if backwards { actions.len() - 1 } else { 1 };
        self.focus = (1..=actions.len())
            .map(|step| actions[(index + step * offset) % actions.len()])
            .find(|action| action.enabled(self.page, self.document.page_count()))
            .unwrap_or(Open);
    }
}

impl ShellSession {
    fn editor_pdf_render_viewport(&self) -> Option<[u16; 2]> {
        let canvas = self.editor_current_layout().canvas;
        (!canvas.is_empty()).then_some([canvas.width, canvas.height])
    }

    pub(in crate::session) fn editor_pdf_has_background_work(&self) -> bool {
        if self.content_screen() != ShellScreen::Editor
            || self.pages.editor.editor_load_state.is_some()
        {
            return false;
        }
        self.pages
            .editor
            .editor_pdf
            .as_ref()
            .is_some_and(|preview| {
                self.editor_pdf_render_viewport()
                    .is_some_and(|size| preview.viewport != Some(size) || preview.task.busy())
            })
    }

    pub(super) fn poll_editor_pdf_preview(&mut self) -> bool {
        if self.pages.editor.editor_pdf.is_none() || self.pages.editor.editor_load_state.is_some() {
            return false;
        }
        let Some(viewport) = self.editor_pdf_render_viewport() else {
            return false;
        };
        let [width, height] = self.terminal_image_cell_size;
        let session_ui = &mut self.ui;
        let preview = session_ui.pages.editor.editor_pdf.as_mut().unwrap();
        if preview.viewport != Some(viewport) {
            preview.viewport = Some(viewport);
            preview.result = None;
            let document = preview.document.clone();
            let page = preview.page;
            let id = super::tasks::next_editor_task_id();
            let submitted = session_ui
                .editor_task_runtime
                .shared
                .task_group
                .as_ref()
                .is_some_and(|group| {
                    preview
                        .task
                        .submit_in(group, move |_| {
                            document
                                .render_page(page, viewport, [width, height])
                                .map(|image| (id, image))
                        })
                        .is_ok()
                });
            if !submitted {
                preview.result = Some(Err(
                    i18n::msg!("editor-pdf-render-error", page = page + 1).into()
                ));
            }
            return true;
        }
        if let Some(result) = preview.task.take() {
            preview.result = Some(result);
            return true;
        }
        false
    }

    pub(super) fn handle_editor_pdf_key(&mut self, key: &KeyInput, platform: &dyn Platform) {
        if !key.phase.is_press_like() {
            return;
        }
        let shortcut = key.modifiers.is_control()
            || (platform.kind() == PlatformKind::Macos && key.modifiers.super_key);
        if shortcut
            && matches!(key.key, InputKey::Char('o' | 'O'))
            && key.phase == InputPhase::Press
        {
            self.open_editor_picker(platform);
            return;
        }
        if key.has_non_shift_modifier() {
            return;
        }
        let preview = self.pages.editor.editor_pdf.as_mut().unwrap();
        match key.key {
            InputKey::PageUp => preview.turn(Previous),
            InputKey::PageDown => preview.turn(Next),
            InputKey::Tab | InputKey::BackTab => {
                preview.move_focus(key.key == InputKey::BackTab || key.modifiers.shift);
            }
            InputKey::Enter | InputKey::Space if key.phase == InputPhase::Press => {
                if preview.focus == Open {
                    self.open_editor_picker(platform);
                } else {
                    preview.turn(preview.focus);
                }
            }
            InputKey::Escape if key.phase == InputPhase::Press => {
                self.request_editor_close(platform)
            }
            _ => {}
        }
    }

    pub(super) fn handle_editor_pdf_pointer(&mut self, mouse: MouseInput, platform: &dyn Platform) {
        if !matches!(mouse.kind, ui::MouseEventKind::Down(PointerButton::Left))
            || self.interaction.completed_button_click_at.is_none()
        {
            return;
        }
        match self
            .editor_current_layout()
            .hit_test(mouse.column(), mouse.row())
        {
            Some(ui::EditorHitTarget::Pdf(Open)) => self.open_editor_picker(platform),
            Some(ui::EditorHitTarget::Pdf(action)) => {
                let preview = self.pages.editor.editor_pdf.as_mut().unwrap();
                preview.focus = action;
                preview.turn(action);
            }
            _ => {}
        }
    }
}
