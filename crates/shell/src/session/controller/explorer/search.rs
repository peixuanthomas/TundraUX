use crate::session::*;

impl ShellSession {
    pub(super) fn sync_explorer_search(&mut self) {
        let request = self
            .app
            .explorer_state()
            .and_then(ExplorerState::subfolder_search)
            .zip(self.app.auth_session().cloned());
        if self.pages.explorer.search_request == request {
            return;
        }
        self.pages.explorer.search_request = request.clone();
        let Some((request, auth)) = request else {
            self.pages.explorer.search_task.cancel();
            return;
        };
        let Some(group) = self.settings_task_runtime.shared.task_group.as_ref() else {
            self.report_explorer_task_error(i18n::tr!("app-tasks-worker-stopped"));
            return;
        };
        let platform = self
            .settings_task_runtime
            .shared
            .platform
            .clone()
            .unwrap_or_else(|| std::sync::Arc::from(platform::native_platform()));
        let controller = self.app.explorer_controller();
        let language = self.language.clone();
        if let Err(error) = self
            .pages
            .explorer
            .search_task
            .submit_in(group, move |cancelled| {
                let _language = i18n::enter_snapshot(language);
                controller.search_subfolders(&request, &auth, platform.as_ref(), cancelled)
            })
        {
            self.report_explorer_task_error(error.to_string());
        }
    }

    pub(super) fn poll_explorer_search(&mut self) -> bool {
        let mut changed = false;
        for id in self.pages.explorer.ids() {
            let Some(page) = self.pages.explorer.get(id) else {
                continue;
            };
            let Some(result) = page.search_task.take() else {
                continue;
            };
            self.with_page_context(id, |session| {
                let expected = session
                    .app
                    .explorer_state()
                    .and_then(ExplorerState::subfolder_search)
                    .zip(session.app.auth_session().cloned());
                if expected.is_some() && expected == session.pages.explorer.search_request {
                    session.update_explorer_state(|state| {
                        state.subfolder_results = Some(result);
                        state.apply_projection();
                    });
                    changed = true;
                }
            });
        }
        changed
    }
}
