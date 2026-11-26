use super::*;
use app::background::Latest;
use ui::{EncodedWorkspaceImage, TerminalImageCache, WorkspaceImageRequest};

#[derive(Default)]
pub(in crate::session) struct PaneImages {
    pub(in crate::session) cache: TerminalImageCache,
    request: Option<WorkspaceImageRequest>,
    encoding: Latest<EncodedWorkspaceImage>,
}

impl PaneImages {
    pub(in crate::session) fn ready(&self) -> bool {
        self.encoding.ready()
    }

    pub(super) fn render(
        &mut self,
        frame: &mut Frame<'_>,
        area: Rect,
        images: &[workspace_protocol::Image],
        picker: &ui::EditorImagePicker,
        group: Option<&ManagedTaskGroup>,
    ) {
        let Some(group) = group else {
            self.cache.render_workspace(frame, area, images, picker);
            return;
        };
        let mut result = self.encoding.take();
        let completed = result.is_some();
        let mut current_visible = false;
        let mut next = None;
        self.cache.render_workspace_with(
            frame,
            area,
            images,
            picker,
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
            self.request = self
                .encoding
                .submit_in(group, move |_| work.encode())
                .is_ok()
                .then_some(request);
        } else if self.request.take().is_some() {
            self.encoding.cancel();
        }
    }
}
