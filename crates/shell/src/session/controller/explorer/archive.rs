use crate::session::*;
use app::explorer::archive::{self, ArchiveFormat};
use app::explorer::{ExplorerConflict, ExplorerConflictAction};
use app::explorer_tasks::{
    ArchiveInputs, ExplorerArchivePlan, ExplorerCollisionResolution, ExplorerTaskPlan,
};
use std::ffi::OsString;
use std::path::PathBuf;
use ui::components::{FormAction, FormDialog, FormEvent, FormField};

const FORMATS: [(ArchiveFormat, &str); 5] = [
    (ArchiveFormat::Zip, "ZIP"),
    (ArchiveFormat::SevenZip, "7z"),
    (ArchiveFormat::Rar, "RAR"),
    (ArchiveFormat::Tar, "TAR"),
    (ArchiveFormat::TarGz, "TAR.GZ"),
];

#[derive(Clone, PartialEq, Eq)]
pub(in crate::session) struct ExplorerArchiveDialog {
    pub(in crate::session) form: FormDialog,
    inputs: ArchiveInputs,
    format: ArchiveFormat,
    original_target: PathBuf,
    replace_identity: Option<platform::FileIdentity>,
}

impl std::fmt::Debug for ExplorerArchiveDialog {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ExplorerArchiveDialog")
            .field("inputs", &self.inputs)
            .field("format", &self.format)
            .finish_non_exhaustive()
    }
}

fn password_field() -> FormField {
    FormField {
        secret: true,
        ..FormField::text("password", i18n::tr!("explorer-archive-password"), "")
    }
}

fn archive_output_path(target: PathBuf, format: ArchiveFormat) -> Option<PathBuf> {
    let name = target.file_name()?;
    if ArchiveFormat::from_path(&target) == Some(format) {
        return Some(target);
    }
    let mut stem = archive::default_extract_name(&target).unwrap_or_else(|| name.to_owned());
    stem.push(".");
    stem.push(format.extension());
    Some(target.with_file_name(stem))
}

impl ExplorerArchiveDialog {
    fn destination(&self) -> Option<PathBuf> {
        let value = &self.form.fields[0].value;
        if value.is_empty() {
            return None;
        }
        let target = if value == &self.original_target.to_string_lossy() {
            self.original_target.clone()
        } else {
            self.original_target.parent()?.join(value)
        };
        match self.inputs {
            ArchiveInputs::Create(_) => archive_output_path(target, self.format),
            ArchiveInputs::Extract(_) => Some(target),
        }
    }

    fn update_format(&mut self) {
        if !matches!(self.inputs, ArchiveInputs::Create(_)) {
            return;
        }
        let Some(&(format, _)) = FORMATS
            .iter()
            .find(|(_, label)| *label == self.form.fields[1].value)
        else {
            return;
        };
        if format == self.format {
            return;
        }
        let field = &mut self.form.fields[0];
        let unchanged = field.value == self.original_target.to_string_lossy();
        let target = if unchanged {
            self.original_target.clone()
        } else {
            PathBuf::from(&field.value)
        };
        if let Some(target) = archive_output_path(target, format) {
            field.value = target.to_string_lossy().into_owned();
            field.input = None;
            if unchanged {
                self.original_target = target;
            }
        }
        if !format.supports_password() {
            self.form.fields.truncate(2);
        } else if self.form.fields.len() == 2 {
            self.form.fields.push(password_field());
        }
        self.format = format;
    }
}

impl ShellSession {
    pub(in crate::session) fn open_archive_creation(&mut self) {
        let Some(state) = self.app.explorer_state() else {
            return;
        };
        let paths = state.effective_selected_paths();
        let Some(directory) = state.current_location.path() else {
            return;
        };
        if paths.is_empty() || state.operation.is_some() {
            return;
        }
        let mut name = if paths.len() == 1 {
            paths[0].file_name()
        } else {
            directory.file_name()
        }
        .map(OsString::from)
        .unwrap_or_else(|| OsString::from(i18n::tr!("explorer-archive-default-name")));
        name.push(".zip");
        let target = directory.join(name);
        let mut format =
            FormField::text("format", i18n::tr!("explorer-archive-format"), FORMATS[0].1);
        format.choices = FORMATS
            .iter()
            .map(|(_, label)| (*label).to_owned())
            .collect();
        let mut form = FormDialog::new(
            "explorer-archive-create",
            i18n::tr!("explorer-archive-create-menu"),
            vec![
                FormField::text(
                    "destination",
                    i18n::tr!("explorer-archive-save-as"),
                    target.to_string_lossy(),
                ),
                format,
                password_field(),
            ],
        );
        form.submit_label = Some(i18n::tr!("explorer-archive-create"));
        self.pages.explorer.archive_dialog = Some(ExplorerArchiveDialog {
            form,
            inputs: ArchiveInputs::Create(paths),
            format: ArchiveFormat::Zip,
            original_target: target,
            replace_identity: None,
        });
    }

    pub(in crate::session) fn open_archive_extraction(
        &mut self,
        source: PathBuf,
        destination: Option<PathBuf>,
        password_retry: Option<bool>,
    ) {
        let Some(format) = ArchiveFormat::from_path(&source) else {
            self.report_explorer_task_error(i18n::msg!(
                "explorer-archive-unsupported",
                path = source.display().to_string()
            ));
            return;
        };
        if self
            .app
            .explorer_state()
            .is_some_and(|state| state.operation.is_some())
        {
            self.notify_status(i18n::msg!(
                "shell-another-explorer-file-operation-is-still-running"
            ));
            return;
        }
        let target = destination.unwrap_or_else(|| {
            source.with_file_name(
                archive::default_extract_name(&source)
                    .unwrap_or_else(|| OsString::from(i18n::tr!("explorer-archive-default-name"))),
            )
        });
        let mut fields = vec![FormField::text(
            "destination",
            i18n::tr!("explorer-archive-extract-to"),
            target.to_string_lossy(),
        )];
        if format.supports_password() {
            fields.push(password_field());
        }
        let mut form = FormDialog::new(
            "explorer-archive-extract",
            i18n::tr!("explorer-archive-extract-menu"),
            fields,
        );
        form.submit_label = Some(i18n::tr!("explorer-archive-extract"));
        if let Some(incorrect) = password_retry {
            form.message = i18n::tr!(if incorrect {
                "explorer-archive-wrong-password"
            } else {
                "explorer-archive-needs-password"
            });
            form.selected = 1;
        }
        self.pages.explorer.archive_dialog = Some(ExplorerArchiveDialog {
            form,
            inputs: ArchiveInputs::Extract(source),
            format,
            original_target: target,
            replace_identity: None,
        });
    }

    pub(in crate::session) fn handle_archive_input(&mut self, event: InputEvent) {
        let main = self.touch_main();
        let Some(dialog) = self.pages.explorer.archive_dialog.as_mut() else {
            return;
        };
        let outcome = match event {
            InputEvent::Mouse(mouse) => dialog.form.handle_confirmed_pointer(mouse, main),
            event => dialog.form.handle_event(event, main),
        };
        dialog.update_format();
        match outcome {
            FormEvent::Action(FormAction::Submit) => self.submit_archive_dialog(None),
            FormEvent::Action(FormAction::Cancel) => {
                self.pages.explorer.archive_dialog = None;
            }
            _ => {}
        }
    }

    pub(in crate::session) fn resolve_archive_conflict(&mut self, action: ExplorerConflictAction) {
        let _ = self.update_explorer_state(|state| state.pending_conflict = None);
        match action {
            ExplorerConflictAction::KeepBoth => {
                self.submit_archive_dialog(Some(ExplorerCollisionResolution::KeepBoth))
            }
            ExplorerConflictAction::Replace => {
                self.submit_archive_dialog(Some(ExplorerCollisionResolution::Replace))
            }
            ExplorerConflictAction::Skip | ExplorerConflictAction::Cancel => {
                self.pages.explorer.archive_dialog = None
            }
        }
    }

    fn submit_archive_dialog(&mut self, decision: Option<ExplorerCollisionResolution>) {
        let Some(dialog) = &self.pages.explorer.archive_dialog else {
            return;
        };
        let Some(target) = dialog
            .destination()
            .filter(|path| path.file_name().is_some())
        else {
            self.pages
                .explorer
                .archive_dialog
                .as_mut()
                .unwrap()
                .form
                .message = i18n::tr!("explorer-archive-destination");
            return;
        };
        let checks = self
            .preflight_explorer_permissions(PermissionAction::ReadFile, dialog.inputs.paths())
            .and_then(|()| {
                self.preflight_explorer_permissions(
                    PermissionAction::WriteFile,
                    std::slice::from_ref(&target),
                )
            });
        if let Err(error) = checks {
            self.report_explorer_task_error(error);
            return;
        }
        if decision == Some(ExplorerCollisionResolution::Replace)
            && let Err(error) = self.preflight_explorer_permissions(
                PermissionAction::DeleteFile,
                std::slice::from_ref(&target),
            )
        {
            self.report_explorer_task_error(error);
            return;
        }
        if decision.is_none()
            && self
                .app
                .explorer_state()
                .is_some_and(|state| state.confirm_name_conflicts)
            && std::fs::symlink_metadata(&target).is_ok()
        {
            let identity = match platform::file_identity(&target) {
                Ok(identity) => identity,
                Err(error) => {
                    self.report_explorer_task_error(error.to_string());
                    return;
                }
            };
            let source = dialog.inputs.paths()[0].clone();
            self.pages
                .explorer
                .archive_dialog
                .as_mut()
                .unwrap()
                .replace_identity = Some(identity);
            self.pages.explorer.explorer_overlay_selection = 0;
            self.pages.explorer.explorer_conflict_apply_to_remaining = false;
            let _ = self.update_explorer_state(|state| {
                state.pending_conflict = Some(ExplorerConflict {
                    source,
                    target,
                    remaining: 1,
                })
            });
            return;
        }
        let dialog = self.pages.explorer.archive_dialog.take().unwrap();
        let password = dialog
            .form
            .fields
            .into_iter()
            .find(|field| field.id == "password" && !field.value.is_empty())
            .map(|field| field.value);
        let collision = decision.unwrap_or(ExplorerCollisionResolution::KeepBoth);
        let replace_identity = if collision == ExplorerCollisionResolution::Replace {
            dialog.replace_identity
        } else {
            None
        };
        self.submit_explorer_task(
            ExplorerTaskPlan::Archive(ExplorerArchivePlan {
                inputs: dialog.inputs,
                format: dialog.format,
                target,
                password,
                collision,
                replace_identity,
            }),
            ShellExplorerTaskKind::Archive,
        );
    }
}
