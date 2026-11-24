use super::*;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PackageTarget {
    id: String,
    name: String,
}

impl PackageTarget {
    fn label(&self) -> String {
        if self.name == self.id {
            self.name.clone()
        } else {
            format!("{} ({})", self.name, self.id)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum PackageChoice {
    Packages(Vec<PackageTarget>),
    Commands(String),
}

impl ShellSession {
    pub(super) fn package_install_targets(
        &self,
        command: Option<&ManagementCommand>,
    ) -> Vec<PackageTarget> {
        let Some(command) = command.filter(|command| {
            command.kind == ManagementKind::Packages
                && matches!(command.action.as_str(), "install" | "pacman_install")
        }) else {
            return Vec::new();
        };
        let rows = &self.pages.management.management_state.snapshot.rows;
        platform::management::selection::expand(command)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|command| {
                let id = command.target?;
                let name = rows
                    .iter()
                    .find(|row| row.id == id)
                    .and_then(|row| row.cells.first())
                    .or_else(|| command.identity.get("name"))
                    .cloned()
                    .unwrap_or_else(|| id.clone());
                Some(PackageTarget { id, name })
            })
            .collect()
    }

    fn installed_launcher_targets(&self) -> Vec<PackageTarget> {
        let state = &self.pages.management.management_state;
        if state.kind != Some(ManagementKind::Packages)
            || !state
                .query
                .as_ref()
                .is_some_and(|query| matches!(query.scope.as_str(), "" | "installed"))
        {
            return Vec::new();
        }
        self.selected_management_rows()
            .into_iter()
            .filter(|row| {
                row.identity
                    .get("installed_version")
                    .is_some_and(|version| !version.is_empty())
            })
            .map(|row| PackageTarget {
                id: row.id.clone(),
                name: row.cells.first().unwrap_or(&row.id).clone(),
            })
            .collect()
    }

    pub(super) fn package_launcher_actions(
        &self,
    ) -> Vec<(ManagementAction, Option<ManagementRow>)> {
        if !self.can_manage_launcher() || !self.can_execute_command_line() {
            return Vec::new();
        }
        let state = &self.pages.management.management_state;
        let installed = self.installed_launcher_targets();
        let mut actions = Vec::new();
        if state.outcome.is_some()
            && !state.just_installed.is_empty()
            && state.just_installed != installed
        {
            let label = if installed.is_empty() {
                i18n::tr!("shell-add-to-launcher")
            } else {
                i18n::tr!("management-add-recent-to-launcher")
            };
            actions.push((
                ManagementAction {
                    id: "installed_result_launcher".into(),
                    label,
                    ..Default::default()
                },
                None,
            ));
        }
        if !installed.is_empty() {
            actions.push((
                ManagementAction {
                    id: "package_launcher".into(),
                    label: i18n::tr!("shell-add-to-launcher"),
                    ..Default::default()
                },
                None,
            ));
        }
        actions
    }

    pub(in crate::session) fn has_package_launcher_result(&self, job: &AutoAdminJob) -> bool {
        let state = &self.pages.management.management_state;
        self.active_screen() == ShellScreen::Management
            && state.auto_admin_job.as_ref() == Some(job)
            && !state.just_installed.is_empty()
            && self.can_manage_launcher()
            && self.can_execute_command_line()
    }

    pub(in crate::session) fn add_package_to_launcher(&mut self, recent: bool) {
        if !self.can_manage_launcher() || !self.can_execute_command_line() {
            return;
        }
        let targets = if recent {
            self.pages
                .management
                .management_state
                .just_installed
                .clone()
        } else {
            self.installed_launcher_targets()
        };
        match targets.as_slice() {
            [] => {}
            [target] => self.lookup_package_command(target.clone()),
            _ => {
                let labels = targets.iter().map(PackageTarget::label).collect();
                self.open_package_choice(
                    i18n::tr!("management-choose-launcher-package"),
                    labels,
                    PackageChoice::Packages(targets),
                );
            }
        }
    }

    fn open_package_choice(&mut self, title: String, choices: Vec<String>, purpose: PackageChoice) {
        self.pages.management.management_state.form = Some(ManagementEditor {
            instance: next_management_form_instance(),
            title: title.clone(),
            message: String::new(),
            message_scroll: 0,
            fields: vec![ManagementField {
                id: "entry".into(),
                label: title,
                choices,
                control: FieldControl::SearchableChoice,
                ..Default::default()
            }],
            input_states: Default::default(),
            selected: 0,
            purpose: FormPurpose::PackageChoice(purpose),
        });
        self.open_management_choice_field(0);
    }

    pub(super) fn submit_package_choice(&mut self, choice: PackageChoice, value: &str) {
        match choice {
            PackageChoice::Packages(targets) => {
                if let Some(target) = targets.into_iter().find(|target| target.label() == value) {
                    self.lookup_package_command(target);
                }
            }
            PackageChoice::Commands(name) => self.open_package_launcher_form(name, value.into()),
        }
    }

    fn lookup_package_command(&mut self, target: PackageTarget) {
        let state = &self.pages.management.management_state;
        let started = self
            .settings_task_runtime
            .shared
            .task_group
            .as_ref()
            .is_some_and(|group| {
                state
                    .package_lookup
                    .submit_in(group, move |cancelled| {
                        platform::management::package_launch::commands(&target.id, cancelled)
                            .unwrap_or_default()
                    })
                    .is_ok()
            });
        if !started {
            self.open_package_launcher_form(target.name, String::new());
            return;
        }
        self.pages.management.management_state.form = Some(ManagementEditor {
            instance: next_management_form_instance(),
            title: i18n::tr!("shell-add-to-launcher"),
            message: i18n::tr!("management-looking-up-launcher", name = target.name.clone()),
            message_scroll: 0,
            fields: Vec::new(),
            input_states: Default::default(),
            selected: 0,
            purpose: FormPurpose::PackageLookup(target.name),
        });
    }

    pub(in crate::session) fn poll_package_launcher(&mut self) -> bool {
        if self.active_screen() != ShellScreen::Management {
            return false;
        }
        let state = &mut self.pages.management.management_state;
        let Some(ManagementEditor {
            purpose: FormPurpose::PackageLookup(name),
            ..
        }) = &state.form
        else {
            return false;
        };
        let Some(mut commands) = state.package_lookup.take() else {
            return false;
        };
        let name = name.clone();
        state.form = None;
        self.reset_management_form_view();
        if commands.len() > 1 {
            self.open_package_choice(
                i18n::tr!("management-choose-launcher-command", name = name.clone()),
                commands,
                PackageChoice::Commands(name),
            );
        } else {
            self.open_package_launcher_form(name, commands.pop().unwrap_or_default());
        }
        true
    }
}
