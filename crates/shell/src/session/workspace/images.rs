use super::*;
use app::background::Latest;
use ui::{EncodedWorkspaceImage, TerminalImageCache, WorkspaceImageRequest};

impl Workspace {
    pub(in crate::session) fn image_scroll(
        &self,
    ) -> Option<(SessionId, workspace_protocol::ImageScroll)> {
        let pane = self
            .layout
            .panes
            .iter()
            .find(|pane| Some(pane.id) == self.selection.focused())?;
        // Terminal scrolling moves whole rows, including any pane alongside this one.
        if pane.content.width.checked_add(2)? != self.width || self.has_overlay() {
            return None;
        }
        let frame = self.windows.get(&pane.id)?.frame.as_ref()?;
        let mut scroll = frame.image_scroll?;
        let bottom = scroll.top.checked_add(scroll.height)?;
        if frame.layout != self.generation
            || frame.modal
            || frame.size != Size::new(pane.content.width, pane.content.height)
            || bottom > pane.content.height
            || frame.images.iter().any(|image| {
                image.rows.is_none()
                    && image.area.y < bottom
                    && image.area.y.saturating_add(image.area.height) > scroll.top
            })
            || frame.headings.iter().any(|heading| {
                let end = heading.area.y.saturating_add(heading.area.height);
                [scroll.top, bottom]
                    .into_iter()
                    .any(|edge| heading.area.y < edge && edge < end)
            })
        {
            return None;
        }
        scroll.top = scroll.top.checked_add(pane.content.y)?;
        Some((pane.id, scroll))
    }
}

#[derive(Default)]
pub(in crate::session) struct PaneImages {
    pub(in crate::session) cache: TerminalImageCache,
    request: Option<WorkspaceImageRequest>,
    encoding: Latest<EncodedWorkspaceImage>,
}

impl PaneImages {
    pub(in crate::session) fn ready(&self) -> bool {
        self.encoding.ready() || (self.request.is_some() && !self.encoding.busy())
    }

    pub(super) fn render(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        images: &[workspace_protocol::Image],
        picker: &ui::EditorImagePicker,
        background: ratatui::style::Color,
        group: Option<&ManagedTaskGroup>,
    ) {
        let Some(group) = group else {
            self.cache.render_workspace(frame, area, images, picker);
            return;
        };
        let mut result = self.encoding.take().or_else(|| {
            (self.request.is_some() && !self.encoding.busy()).then(EncodedWorkspaceImage::default)
        });
        let completed = result.is_some();
        let mut current_visible = false;
        let mut next = None;
        self.cache.render_workspace_with(
            frame,
            area,
            images,
            picker,
            background,
            Some(&mut |request| {
                if self.request.as_ref() == Some(request) {
                    current_visible = true;
                    result.take()
                } else {
                    next.get_or_insert_with(|| request.clone());
                    None
                }
            }),
        );
        if current_visible && !completed && self.encoding.busy() {
            return;
        }
        if let Some(request) = next {
            let work = request.clone();
            self.request = Some(request);
            if self
                .encoding
                .submit_in(group, move |_| work.encode())
                .is_err()
            {
                platform::task_events::notify();
            }
        } else if self.request.take().is_some() {
            self.encoding.cancel();
        }
    }
}
