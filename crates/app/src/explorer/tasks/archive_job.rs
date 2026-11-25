use super::*;
use crate::explorer::archive::{self, ArchiveError, ArchiveFormat};
use platform::{FileIdentity, file_identity, rename_path_no_replace};
use workspace_protocol::NativePath;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArchiveInputs {
    Create(Vec<PathBuf>),
    Extract(PathBuf),
}

impl ArchiveInputs {
    pub fn paths(&self) -> &[PathBuf] {
        match self {
            Self::Create(paths) => paths,
            Self::Extract(path) => std::slice::from_ref(path),
        }
    }

    pub fn operation(&self) -> ExplorerTaskOperation {
        match self {
            Self::Create(_) => ExplorerTaskOperation::Compress,
            Self::Extract(_) => ExplorerTaskOperation::Extract,
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub struct ExplorerArchivePlan {
    pub inputs: ArchiveInputs,
    pub format: ArchiveFormat,
    pub target: PathBuf,
    pub password: Option<String>,
    pub collision: ExplorerCollisionResolution,
    pub replace_identity: Option<FileIdentity>,
}

impl fmt::Debug for ExplorerArchivePlan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ExplorerArchivePlan")
            .field("inputs", &self.inputs)
            .field("format", &self.format)
            .field("target", &self.target)
            .field("collision", &self.collision)
            .finish_non_exhaustive()
    }
}

fn archive_error(error: ArchiveError, path: &Path) -> ExplorerTaskError {
    let message = match error {
        ArchiveError::PasswordRequired | ArchiveError::InvalidPassword => {
            return ExplorerTaskError::ArchivePassword {
                incorrect: matches!(error, ArchiveError::InvalidPassword),
            };
        }
        ArchiveError::Cancelled => return ExplorerTaskError::Cancelled,
        ArchiveError::Io(error) => return io_error("archive", path, error),
        ArchiveError::InvalidArchive => msg!("explorer-archive-invalid"),
        ArchiveError::UnsafePath(path) => {
            msg!("explorer-archive-unsafe", path = path.display().to_string())
        }
        ArchiveError::DuplicatePath(path) => msg!(
            "explorer-archive-duplicate",
            path = path.display().to_string()
        ),
        ArchiveError::UnsupportedEntry(path) => {
            msg!("explorer-archive-entry", path = path.display().to_string())
        }
        ArchiveError::UnsupportedEncryption => msg!("explorer-archive-no-encryption"),
    };
    ExplorerTaskError::InvalidPlan {
        message: message.into(),
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct ArchiveCheckpoint {
    staging: NativePath,
    directory_id: FileIdentity,
    target: NativePath,
    output_id: Option<FileIdentity>,
    replaced_id: Option<FileIdentity>,
}

impl ExecutionContext<'_> {
    pub(super) fn run_archive(
        &mut self,
        plan: ExplorerArchivePlan,
    ) -> Result<(), ExplorerTaskError> {
        let invalid_destination = || ExplorerTaskError::InvalidPlan {
            message: msg!("explorer-archive-destination").into(),
        };
        let name = plan.target.file_name().ok_or_else(invalid_destination)?;
        let parent = plan.target.parent().ok_or_else(invalid_destination)?;
        let parent = fs::canonicalize(parent)
            .map_err(|e| io_error("resolve archive directory", parent, e))?;
        let mut target = parent.join(name);
        let source = plan
            .inputs
            .paths()
            .first()
            .ok_or_else(invalid_destination)?;
        for path in plan.inputs.paths() {
            let canonical =
                fs::canonicalize(path).map_err(|e| io_error("resolve archive input", path, e))?;
            let same_object =
                path_exists_no_follow(&target) && file_identity(&target)? == file_identity(path)?;
            if same_object || target.starts_with(&canonical) || canonical.starts_with(&target) {
                return Err(invalid_destination());
            }
        }
        if path_exists_no_follow(&target) {
            match plan.collision {
                ExplorerCollisionResolution::KeepBoth => {
                    target = archive_keep_both(&target, &plan);
                }
                ExplorerCollisionResolution::Replace
                    if Some(file_identity(&target)?) == plan.replace_identity => {}
                ExplorerCollisionResolution::Replace => {
                    return Err(ExplorerTaskError::DestinationChanged { path: target });
                }
                ExplorerCollisionResolution::Skip => {
                    self.record_skip(source, Some(&target));
                    return Ok(());
                }
                ExplorerCollisionResolution::Cancel => return Err(ExplorerTaskError::Cancelled),
            }
        } else if plan.replace_identity.is_some() {
            return Err(ExplorerTaskError::DestinationChanged { path: target });
        }

        let mut builder = tempfile::Builder::new();
        builder.prefix(".tundra-stage-archive-");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            builder.permissions(fs::Permissions::from_mode(0o700));
        }
        let stage = builder
            .tempdir_in(&parent)
            .map_err(|e| io_error("create archive staging directory", &parent, e))?;
        let mut checkpoint = ArchiveCheckpoint {
            directory_id: file_identity(stage.path())?,
            staging: NativePath(stage.path().to_owned()),
            target: NativePath(target.clone()),
            output_id: None,
            replaced_id: plan.replace_identity,
        };
        self.checkpoint("archive_work", serde_json::json!(checkpoint))?;
        // The journal owns cleanup from here, including restoration of a replaced destination.
        let stage = stage.keep();
        let output = stage.join("output");
        self.phase(ExplorerTaskPhase::Executing);
        let archive_progress = Arc::new(Mutex::new(self.progress.clone()));
        let progress_state = archive_progress.clone();
        let id = self.id;
        let event_tx = self.event_tx.clone();
        let cancellation = self.cancellation.clone();
        let mut last_report = std::time::Instant::now();
        let progress = move |path: &Path, bytes: u64| {
            let mut current = progress_state.lock().expect("archive task progress lock");
            current.processed_bytes = bytes;
            if last_report.elapsed() >= Duration::from_millis(50) {
                current.current_path = Some(path.to_owned());
                let progress = current.clone();
                drop(current);
                send_event(&event_tx, ExplorerTaskEvent::Progress { id, progress });
                last_report = std::time::Instant::now();
            }
            !cancellation.is_cancelled()
        };
        let result = match &plan.inputs {
            ArchiveInputs::Create(sources) => archive::create(
                plan.format,
                sources,
                &output,
                plan.password.as_deref(),
                progress,
            ),
            ArchiveInputs::Extract(source) => archive::extract(
                plan.format,
                source,
                &output,
                plan.password.as_deref(),
                progress,
            ),
        };
        self.progress = archive_progress
            .lock()
            .expect("archive task progress lock")
            .clone();
        result.map_err(|e| archive_error(e, source))?;
        self.check_cancel()?;
        checkpoint.output_id = Some(file_identity(&output)?);
        self.checkpoint("archive_publish", serde_json::json!(checkpoint))?;
        if let Some(expected) = plan.replace_identity {
            if file_identity(&target)? != expected {
                return Err(ExplorerTaskError::DestinationChanged { path: target });
            }
            let old = stage.join("previous");
            rename_path_no_replace(&target, &old)?;
            if file_identity(&old)? != expected {
                rename_path_no_replace(&old, &target)?;
                return Err(ExplorerTaskError::DestinationChanged { path: target });
            }
        }
        rename_path_no_replace(&output, &target)?;
        self.phase(ExplorerTaskPhase::CleaningUp);
        fs::remove_dir_all(&stage)
            .map_err(|e| io_error("clean archive staging directory", &stage, e))?;
        self.summary.processed_bytes = self.progress.processed_bytes;
        self.summary.total_bytes = self.progress.processed_bytes;
        self.progress.total_bytes = self.progress.processed_bytes;
        self.record_success(source, Some(&target));
        Ok(())
    }
}

fn archive_keep_both(target: &Path, plan: &ExplorerArchivePlan) -> PathBuf {
    let filename = target.file_name().expect("validated archive destination");
    let stem = match plan.inputs {
        ArchiveInputs::Create(_) => {
            archive::default_extract_name(target).unwrap_or_else(|| filename.to_owned())
        }
        ArchiveInputs::Extract(_) => filename.to_owned(),
    };
    let extension = std::str::from_utf8(&filename.as_encoded_bytes()[stem.len()..])
        .expect("recognized archive extensions are ASCII");
    for index in 2_u64.. {
        let mut name = stem.clone();
        name.push(format!(" ({index})"));
        name.push(extension);
        let candidate = target.with_file_name(name);
        if !path_exists_no_follow(&candidate) {
            return candidate;
        }
    }
    unreachable!("the keep-both suffix space is exhausted")
}

pub(super) fn recover_archive(payload: &serde_json::Value) -> RecoveryOutcome {
    let restored = || RecoveryOutcome::Recovered("archive staging reconciled".into());
    let manual = |reason: String| RecoveryOutcome::ManualActionRequired(reason);
    let Ok(checkpoint) = serde_json::from_value::<ArchiveCheckpoint>(payload.clone()) else {
        return manual("archive checkpoint has no valid staging identity".into());
    };
    let stage = &checkpoint.staging.0;
    let target = &checkpoint.target.0;
    if !path_exists_no_follow(stage) {
        return restored();
    }
    if stage.parent() != target.parent()
        || !stage.file_name().is_some_and(|name| {
            name.as_encoded_bytes()
                .starts_with(b".tundra-stage-archive-")
        })
        || file_identity(stage).ok() != Some(checkpoint.directory_id)
    {
        return manual(format!(
            "archive staging directory changed: {}",
            stage.display()
        ));
    }
    let old = stage.join("previous");
    if path_exists_no_follow(&old) {
        if file_identity(&old).ok() != checkpoint.replaced_id {
            return manual(format!(
                "archive backup identity changed: {}",
                old.display()
            ));
        }
        if !path_exists_no_follow(target) {
            if let Err(error) = rename_path_no_replace(&old, target) {
                return manual(error.to_string());
            }
        } else if file_identity(target).ok() != checkpoint.output_id {
            return manual(format!(
                "archive destination is occupied; original retained at {}",
                old.display()
            ));
        }
    }
    match fs::remove_dir_all(stage) {
        Ok(()) => restored(),
        Err(error) => manual(error.to_string()),
    }
}
