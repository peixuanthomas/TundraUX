use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Default)]
pub(crate) struct BuildIdentity {
    pub(crate) commit: Option<String>,
    pub(crate) dirty: bool,
    pub(crate) inputs: Vec<PathBuf>,
}

pub(crate) fn build_identity(workspace: &Path, explicit: Option<String>) -> BuildIdentity {
    if let Some(commit) = explicit.filter(|value| !value.trim().is_empty()) {
        return BuildIdentity {
            commit: Some(commit),
            ..BuildIdentity::default()
        };
    }
    if !workspace.join(".git").exists() {
        return BuildIdentity::default();
    }

    let mut inputs = git_metadata_inputs(workspace);
    inputs.extend(tracked_files(workspace));
    BuildIdentity {
        commit: git_output(workspace, &["rev-parse", "HEAD"]),
        dirty: tracked_dirty(workspace),
        inputs,
    }
}

fn git_metadata_inputs(workspace: &Path) -> Vec<PathBuf> {
    let Some(paths) = git_output(
        workspace,
        &[
            "rev-parse",
            "--git-path",
            "HEAD",
            "--git-path",
            "index",
            "--git-path",
            "packed-refs",
        ],
    ) else {
        return Vec::new();
    };
    let mut paths = paths.lines().map(|path| workspace.join(path));
    let (Some(head), Some(index), Some(packed_refs)) = (paths.next(), paths.next(), paths.next())
    else {
        return Vec::new();
    };
    let mut inputs = Vec::new();
    let marker = workspace.join(".git");
    if marker.is_file() {
        inputs.push(marker);
    }
    if head.is_file() {
        inputs.push(head.clone());
    }
    inputs.extend(existing_input(&index));
    if let Ok(contents) = std::fs::read_to_string(&head)
        && let Some(reference) = contents.trim().strip_prefix("ref: ")
        && let Some(path) = git_output(workspace, &["rev-parse", "--git-path", reference])
    {
        let loose_ref = workspace.join(path);
        if !loose_ref.is_file() && packed_refs.is_file() {
            inputs.push(packed_refs);
        }
        inputs.extend(existing_input(&loose_ref));
    }
    inputs
}

fn existing_input(path: &Path) -> Option<PathBuf> {
    // A missing input forces Cargo to rerun, so watch its parent for creation.
    path.ancestors()
        .find(|path| path.exists())
        .map(Path::to_owned)
}

fn tracked_files(workspace: &Path) -> Vec<PathBuf> {
    let Ok(output) = Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(workspace)
        .output()
    else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }
    output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .filter_map(|relative| std::str::from_utf8(relative).ok())
        .map(|relative| workspace.join(relative))
        .collect()
}

fn tracked_dirty(workspace: &Path) -> bool {
    Command::new("git")
        .args([
            "--no-optional-locks",
            "status",
            "--porcelain",
            "--untracked-files=no",
        ])
        .current_dir(workspace)
        .output()
        .map(|output| output.status.success() && !output.stdout.is_empty())
        .unwrap_or(false)
}

fn git_output(workspace: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(workspace)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}
