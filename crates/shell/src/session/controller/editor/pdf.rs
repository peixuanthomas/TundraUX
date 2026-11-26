use std::collections::VecDeque;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use app::editor::{PdfPageImage, PdfPreviewDocument};
use i18n::LocalizedText;
use ui::EditorPdfAction::{self, Jump, Next, Open, Previous, Rotate, ZoomIn, ZoomOut};
use ui::components::{FormAction, FormDialog, FormEvent, FormField};
use ui::{EditorPdfPageImage, EditorPreviewZoom};

use crate::session::*;

const MAX_CACHED_PAGES: usize = 12;
const MAX_CACHE_BYTES: usize = 32 * 1024 * 1024;
const PREFETCH_RADIUS: usize = 2;
const RENDER_BATCH_PAGES: usize = 3;

#[cfg(test)]
#[path = "../../../../tests/unit/session/controller/editor/pdf_cache_tests.rs"]
mod tests;

type PageRender = Result<(u64, EditorPdfPageImage), LocalizedText>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PdfViewport {
    cells: [u16; 2],
    cell_size: [u16; 2],
    quarter_turns: u8,
}

#[derive(Debug)]
struct RenderBatch {
    pages: VecDeque<usize>,
    completed: Receiver<PageRender>,
}

#[derive(Debug, Default)]
struct PdfRenderTask {
    current: Option<usize>,
    visible: app::background::Latest<PageRender>,
    batch: Option<RenderBatch>,
    prefetch: app::background::Latest<()>,
}

// Page snapshots must not take over a live worker or steal its results.
impl Clone for PdfRenderTask {
    fn clone(&self) -> Self {
        Self::default()
    }
}

impl PartialEq for PdfRenderTask {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl Eq for PdfRenderTask {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(in crate::session) struct EditorPdfPreview {
    document: PdfPreviewDocument,
    page: usize,
    focus: EditorPdfAction,
    zoom: EditorPreviewZoom,
    quarter_turns: u8,
    pub(in crate::session) jump: Option<FormDialog>,
    viewport: Option<PdfViewport>,
    targets: Vec<usize>,
    cache: VecDeque<(usize, PageRender)>,
    task: PdfRenderTask,
}

impl EditorPdfPreview {
    pub(super) fn new(document: PdfPreviewDocument) -> Self {
        Self {
            document,
            page: 0,
            focus: Open,
            zoom: EditorPreviewZoom::default(),
            quarter_turns: 0,
            jump: None,
            viewport: None,
            targets: Vec::new(),
            cache: VecDeque::new(),
            task: Default::default(),
        }
    }

    pub(super) fn view_model(&self, viewport: Option<PdfViewport>) -> ui::EditorPdfViewModel {
        let result = self
            .viewport
            .filter(|current| Some(*current) == viewport)
            .and_then(|_| self.cached(self.page));
        ui::EditorPdfViewModel {
            page: self.page,
            total: self.document.page_count(),
            focus: self.focus,
            zoom: self.zoom,
            jump: self.jump.clone(),
            image: result.and_then(|result| result.as_ref().ok()).cloned(),
            error: result
                .and_then(|result| result.as_ref().err())
                .map(LocalizedText::render_current),
        }
    }

    fn turn(&mut self, action: EditorPdfAction) {
        if !action.enabled(self.page, self.document.page_count(), self.zoom) {
            return;
        }
        let page = match action {
            Previous => self.page - 1,
            Next => self.page + 1,
            _ => return,
        };
        self.go_to(page);
    }

    fn go_to(&mut self, page: usize) {
        if page == self.page {
            return;
        }
        self.page = page;
        self.zoom.center();
        if !self
            .focus
            .enabled(self.page, self.document.page_count(), self.zoom)
        {
            self.focus = Open;
        }
        if let Some(index) = self.cache.iter().position(|(page, _)| *page == self.page) {
            let cached = self.cache.remove(index).unwrap();
            self.cache.push_back(cached);
        }
        self.plan_targets();
    }

    fn cached(&self, page: usize) -> Option<&PageRender> {
        self.cache
            .iter()
            .find_map(|(index, raster)| (*index == page).then_some(raster))
    }

    fn plan_targets(&mut self) {
        self.targets.clear();
        let Some(viewport) = self.viewport else {
            return;
        };
        let total = self.document.page_count();
        let radius = if total <= MAX_CACHED_PAGES {
            total - 1
        } else {
            PREFETCH_RADIUS
        };
        let neighbors = (1..=radius).flat_map(|distance| {
            [
                (self.page + distance < total).then_some(self.page + distance),
                self.page.checked_sub(distance),
            ]
            .into_iter()
            .flatten()
        });
        let mut bytes = 0;
        for page in std::iter::once(self.page).chain(neighbors) {
            let cost = self
                .document
                .rotated_page_size(
                    page,
                    viewport.cells,
                    viewport.cell_size,
                    viewport.quarter_turns,
                )
                .map_or(0, EditorPdfPageImage::estimated_bytes);
            if bytes + cost <= MAX_CACHE_BYTES {
                self.targets.push(page);
                bytes += cost;
            }
        }
    }

    fn record(&mut self, page: usize, result: PageRender) -> bool {
        if !self.targets.contains(&page) || self.cached(page).is_some() {
            return false;
        }
        let mut bytes = render_bytes(&result)
            + self
                .cache
                .iter()
                .map(|(_, result)| render_bytes(result))
                .sum::<usize>();
        // Keep the current prefetch window intact and evict older visits first.
        while self.cache.len() >= MAX_CACHED_PAGES || bytes > MAX_CACHE_BYTES {
            let index = self
                .cache
                .iter()
                .position(|(page, _)| !self.targets.contains(page))
                .unwrap();
            bytes -= render_bytes(&self.cache.remove(index).unwrap().1);
        }
        self.cache.push_back((page, result));
        page == self.page
    }

    fn poll(
        &mut self,
        viewport: Option<PdfViewport>,
        group: Option<&watchdog::ManagedTaskGroup>,
    ) -> bool {
        let mut changed = self.viewport != viewport;
        if changed {
            self.viewport = viewport;
            self.cache.clear();
            self.task.current = None;
            self.task.visible.cancel();
            self.task.batch = None;
            self.task.prefetch.cancel();
            self.plan_targets();
        }
        let Some(viewport) = viewport else {
            return changed;
        };

        if let Some(page) = self.task.current {
            if let Some(result) = self.task.visible.take() {
                self.task.current = None;
                changed |= self.record(page, result);
            } else if !self.task.visible.busy() {
                self.task.current = None;
                changed |= self.record(page, Err(page_render_error(page)));
            }
        }
        while let Some(batch) = &mut self.task.batch {
            match batch.completed.try_recv() {
                Ok(result) => {
                    let page = batch.pages.pop_front().unwrap();
                    changed |= self.record(page, result);
                }
                Err(TryRecvError::Disconnected) => {
                    self.task.batch = None;
                }
                Err(TryRecvError::Empty) => break,
            }
        }
        self.task.prefetch.take();

        let page = self.page;
        if self.cached(page).is_none() {
            if self.task.current == Some(page) {
                return changed;
            }
            if self
                .task
                .batch
                .as_ref()
                .is_some_and(|batch| batch.pages.front() == Some(&page))
            {
                return changed;
            }
            if self.task.batch.is_some() || self.task.prefetch.busy() {
                if self
                    .task
                    .batch
                    .as_ref()
                    .is_some_and(|batch| batch.pages.contains(&page))
                {
                    self.task.batch = None;
                    self.task.prefetch.cancel();
                }
                // Do not wait for a neighbor or old viewport to finish rasterizing.
                let document = self.document.clone();
                let submitted = group.is_some_and(|group| {
                    self.task
                        .visible
                        .submit_in(group, move |cancel| {
                            let raster = document
                                .render_rotated_pages(
                                    [page],
                                    viewport.cells,
                                    viewport.cell_size,
                                    viewport.quarter_turns,
                                )
                                .next()
                                .unwrap();
                            if cancel.load(Ordering::Relaxed) {
                                return Err(page_render_error(page));
                            }
                            prepare_page(page, raster)
                        })
                        .is_ok()
                });
                self.task.current = submitted.then_some(page);
                if !submitted {
                    changed |= self.record(page, Err(page_render_error(page)));
                }
                return changed;
            }
        }
        if self.task.batch.is_some() {
            return changed;
        }

        let pages = self
            .targets
            .iter()
            .copied()
            .filter(|page| self.cached(*page).is_none() && self.task.current != Some(*page))
            .take(RENDER_BATCH_PAGES)
            .collect::<VecDeque<_>>();
        if pages.is_empty() {
            return changed;
        }

        let document = self.document.clone();
        let requested = pages.clone();
        let (sender, completed) = mpsc::channel();
        let submitted = group.is_some_and(|group| {
            self.task
                .prefetch
                .submit_in(group, move |cancel| {
                    let mut renders = document.render_rotated_pages(
                        requested.iter().copied(),
                        viewport.cells,
                        viewport.cell_size,
                        viewport.quarter_turns,
                    );
                    for page in requested.iter().copied() {
                        if cancel.load(Ordering::Acquire) {
                            break;
                        }
                        let raster = renders.next().unwrap();
                        if cancel.load(Ordering::Acquire) {
                            break;
                        }
                        let result = prepare_page(page, raster);
                        if cancel.load(Ordering::Acquire) || sender.send(result).is_err() {
                            break;
                        }
                        platform::task_events::notify();
                    }
                })
                .is_ok()
        });
        if submitted {
            self.task.batch = Some(RenderBatch { pages, completed });
        } else {
            self.task.batch = None;
            for page in pages {
                changed |= self.record(page, Err(page_render_error(page)));
            }
        }
        changed
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
            .find(|action| action.enabled(self.page, self.document.page_count(), self.zoom))
            .unwrap_or(Open);
    }
}

fn render_bytes(result: &PageRender) -> usize {
    result.as_ref().map_or(0, |(_, image)| image.bytes())
}

fn page_render_error(page: usize) -> LocalizedText {
    i18n::msg!("editor-pdf-render-error", page = page + 1).into()
}

fn prepare_page(page: usize, raster: Result<PdfPageImage, LocalizedText>) -> PageRender {
    let image = EditorPdfPageImage::prepare(raster?).ok_or_else(|| page_render_error(page))?;
    Ok((super::tasks::next_editor_task_id(), image))
}

impl ShellSession {
    fn editor_pdf_canvas(&self) -> Rect {
        let _language = i18n::enter_snapshot(self.language.clone());
        ui::editor_pdf_canvas(self.editor_content_rect())
    }

    pub(super) fn editor_pdf_render_viewport(&self) -> Option<PdfViewport> {
        let preview = self.pages.editor.editor_pdf.as_ref()?;
        let canvas = self.editor_pdf_canvas();
        (!canvas.is_empty()).then_some(PdfViewport {
            cells: preview.zoom.pdf_viewport(canvas),
            cell_size: self.terminal_image_cell_size,
            quarter_turns: preview.quarter_turns,
        })
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
                preview.viewport != self.editor_pdf_render_viewport()
                    || preview.task.current.is_some()
                    || preview.task.batch.is_some()
                    || preview
                        .targets
                        .iter()
                        .any(|page| preview.cached(*page).is_none())
            })
    }

    pub(super) fn poll_editor_pdf_preview(&mut self) -> bool {
        if self.pages.editor.editor_pdf.is_none() || self.pages.editor.editor_load_state.is_some() {
            return false;
        }
        let viewport = self.editor_pdf_render_viewport();
        let session_ui = &mut self.ui;
        let preview = session_ui.pages.editor.editor_pdf.as_mut().unwrap();
        preview.poll(
            viewport,
            session_ui.editor_task_runtime.shared.task_group.as_ref(),
        )
    }

    pub(super) fn handle_editor_pdf_key(&mut self, key: &KeyInput, platform: &dyn Platform) {
        if self
            .pages
            .editor
            .editor_pdf
            .as_ref()
            .unwrap()
            .jump
            .is_some()
        {
            self.handle_editor_pdf_form(InputEvent::Key(key.clone()));
            return;
        }
        if !key.phase.is_press_like() {
            return;
        }
        let shortcut = key.modifiers.is_control()
            || (platform.kind() == PlatformKind::Macos && key.modifiers.super_key);
        if shortcut {
            if key.phase == InputPhase::Press {
                match key.key {
                    InputKey::Char('o' | 'O') => self.open_editor_picker(platform),
                    InputKey::Char('g' | 'G') => self.activate_editor_pdf_action(Jump, platform),
                    _ => {}
                }
            }
            return;
        }
        if key.has_non_shift_modifier() {
            return;
        }
        if let Some(delta) = super::preview_pan_direction(key) {
            let canvas = self.editor_pdf_canvas();
            let cell_size = self.terminal_image_cell_size;
            let preview = self.pages.editor.editor_pdf.as_mut().unwrap();
            if preview.zoom.is_zoomed()
                && let Some(size) = preview.document.rotated_page_size(
                    preview.page,
                    preview.zoom.pdf_viewport(canvas),
                    cell_size,
                    preview.quarter_turns,
                )
            {
                let window = preview.zoom.window(canvas, size.cells.map(u32::from));
                preview.zoom.pan(window, delta);
            }
            return;
        }
        let preview = self.pages.editor.editor_pdf.as_mut().unwrap();
        match key.key {
            InputKey::PageUp => preview.turn(Previous),
            InputKey::PageDown => preview.turn(Next),
            InputKey::Char('+' | '=') => self.activate_editor_pdf_action(ZoomIn, platform),
            InputKey::Char('-') => self.activate_editor_pdf_action(ZoomOut, platform),
            InputKey::Char('r' | 'R') if key.phase == InputPhase::Press => {
                self.activate_editor_pdf_action(Rotate, platform);
            }
            InputKey::Tab | InputKey::BackTab => {
                preview.move_focus(key.key == InputKey::BackTab || key.modifiers.shift);
            }
            InputKey::Enter | InputKey::Space if key.phase == InputPhase::Press => {
                let action = preview.focus;
                self.activate_editor_pdf_action(action, platform);
            }
            InputKey::Escape if key.phase == InputPhase::Press => {
                self.request_editor_close(platform)
            }
            _ => {}
        }
    }

    pub(super) fn handle_editor_pdf_pointer(&mut self, mouse: MouseInput, platform: &dyn Platform) {
        if self
            .pages
            .editor
            .editor_pdf
            .as_ref()
            .unwrap()
            .jump
            .is_some()
        {
            self.handle_editor_pdf_form(InputEvent::Mouse(mouse));
            return;
        }
        if !matches!(mouse.kind, ui::MouseEventKind::Down(PointerButton::Left))
            || self.interaction.completed_button_click_at.is_none()
        {
            return;
        }
        match self
            .editor_current_layout()
            .hit_test(mouse.column(), mouse.row())
        {
            Some(ui::EditorHitTarget::Pdf(action)) => {
                self.pages.editor.editor_pdf.as_mut().unwrap().focus = action;
                self.activate_editor_pdf_action(action, platform);
            }
            _ => {}
        }
    }

    fn activate_editor_pdf_action(&mut self, action: EditorPdfAction, platform: &dyn Platform) {
        let preview = self.pages.editor.editor_pdf.as_mut().unwrap();
        let total = preview.document.page_count();
        if !action.enabled(preview.page, total, preview.zoom) {
            return;
        }
        match action {
            Open => self.open_editor_picker(platform),
            Previous | Next => preview.turn(action),
            Rotate => {
                preview.quarter_turns = (preview.quarter_turns + 1) % 4;
                preview.zoom.center();
            }
            ZoomIn | ZoomOut => {
                preview.zoom.change(action == ZoomIn);
                if !preview.focus.enabled(preview.page, total, preview.zoom) {
                    preview.move_focus(false);
                }
            }
            Jump => {
                preview.focus = Jump;
                let label = i18n::tr!("ui-editor-pdf-jump");
                let mut form = FormDialog::new(
                    "editor-pdf-jump",
                    &label,
                    vec![FormField::text(
                        "page",
                        i18n::tr!("ui-editor-pdf-page-number", total = total),
                        "",
                    )],
                );
                form.submit_label = Some(label);
                preview.jump = Some(form);
            }
        }
    }

    pub(super) fn handle_editor_pdf_form(&mut self, event: InputEvent) {
        let main = self.editor_content_rect();
        let preview = self.pages.editor.editor_pdf.as_mut().unwrap();
        let Some(form) = &mut preview.jump else {
            return;
        };
        let result = match event {
            InputEvent::Mouse(mouse) => form.handle_confirmed_pointer(mouse, main),
            event => form.handle_event(event, main),
        };
        match result {
            FormEvent::Action(FormAction::Cancel) => preview.jump = None,
            FormEvent::Action(FormAction::Submit) => {
                let total = preview.document.page_count();
                let page = form.fields[0].value.trim().parse::<usize>();
                if let Ok(page) = page
                    && (1..=total).contains(&page)
                {
                    preview.jump = None;
                    preview.go_to(page - 1);
                } else {
                    form.message = i18n::tr!("ui-editor-pdf-page-invalid", total = total);
                }
            }
            _ => {}
        }
    }
}
