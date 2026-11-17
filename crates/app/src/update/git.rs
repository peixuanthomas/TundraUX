//! Read public branch and commit metadata without checking out source code.
use super::{
    BuildIdentity, GITHUB_OWNER, GITHUB_REPO, UpdateCancellation, UpdateCheckResult, UpdateCommit,
    UpdateError, UpdateGitCommit, UpdateGitRow, UpdateRelation, WorkDirectory,
};
use platform::ProcessSpec;
#[cfg(test)]
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub(super) fn is_commit_sha(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

pub(super) fn check(
    identity: &BuildIdentity,
    cancel: &UpdateCancellation,
) -> Result<UpdateCheckResult, UpdateError> {
    let platform = platform::native_platform();
    let work = platform.create_temp_dir("update-check").map_err(|error| {
        UpdateError::new(format!("could not create Git check directory: {error}"))
    })?;
    let mut owned = WorkDirectory::new(work.clone());
    cancel.check()?;
    let result = check_in(
        identity,
        &format!("https://github.com/{GITHUB_OWNER}/{GITHUB_REPO}.git"),
        &work,
        cancel,
    );
    // A check never retains its scratch repository, even when it succeeds.
    super::cleanup_update_work(&work)?;
    owned.retain();
    result
}

fn check_in(
    identity: &BuildIdentity,
    remote: &str,
    work: &Path,
    cancel: &UpdateCancellation,
) -> Result<UpdateCheckResult, UpdateError> {
    let git = Git {
        work: work.to_owned(),
        deadline: Instant::now() + Duration::from_secs(60),
        cancel: cancel.clone(),
    };
    // Fetch every branch and its ancestors without checkout or source files.
    git.run(&[
        "clone",
        "--bare",
        "--filter=tree:0",
        "--no-tags",
        // Linux beta updates always follow master, even if the remote default changes.
        #[cfg(target_os = "linux")]
        "--branch=master",
        "--",
        remote,
        "history",
    ])?;
    let default_branch = git.repo(&["symbolic-ref", "--short", "HEAD"])?;
    let head_sha = git.repo(&["rev-parse", "HEAD"])?;
    if !is_commit_sha(&head_sha) {
        return Err(UpdateError::new(
            "Git returned an invalid default branch commit",
        ));
    }
    let git_tree = parse_graph(&git.repo(&[
        "log",
        "--graph",
        "--topo-order",
        "--no-color",
        "--no-notes",
        "--no-show-signature",
        "--encoding=UTF-8",
        "--decorate=short",
        "--decorate-refs=refs/heads/*",
        "--format=%x00%H%x00%D%x00%s",
        "--branches",
        "--",
    ])?)?;
    let (relation, range) = match identity.commit_sha.as_deref() {
        Some(local) if local == head_sha => (UpdateRelation::Identical, None),
        Some(local)
            if git_tree.iter().any(|row| {
                row.commit
                    .as_ref()
                    .is_some_and(|commit| commit.sha == local)
            }) && git.repo(&["rev-parse", "--is-shallow-repository"])? == "false" =>
        {
            let comparison = format!("{local}...{head_sha}");
            let counts = git.repo(&["rev-list", "--left-right", "--count", &comparison])?;
            let mut counts = counts.split_whitespace().map(str::parse);
            let (Some(Ok(local_ahead)), Some(Ok(remote_ahead))) = (counts.next(), counts.next())
            else {
                return Err(UpdateError::new("Git returned invalid commit counts"));
            };
            let relation = match (local_ahead, remote_ahead) {
                (0, _) => UpdateRelation::Behind { remote_ahead },
                (_, 0) => UpdateRelation::Ahead { local_ahead },
                _ => UpdateRelation::Diverged {
                    remote_ahead,
                    local_ahead,
                },
            };
            (relation, Some(format!("{local}..{head_sha}")))
        }
        _ => (UpdateRelation::Unknown, Some(head_sha.clone())),
    };
    let commits = if let Some(range) = range {
        let mut args = vec!["log", "--format=%H%x00%B", "-z"];
        if relation == UpdateRelation::Unknown {
            args.push("--max-count=20");
        } else {
            args.push("--reverse");
        }
        args.extend([&range, "--"]);
        parse_commits(&git.repo(&args)?)?
    } else {
        Vec::new()
    };
    Ok(UpdateCheckResult {
        release: None,
        default_branch,
        head_sha,
        relation,
        commits,
        git_tree: Some(Ok(git_tree.into())),
    })
}

fn parse_graph(output: &str) -> Result<Vec<UpdateGitRow>, UpdateError> {
    output
        .lines()
        .map(|line| {
            let mut parts = line.splitn(4, '\0');
            let graph = parts.next().unwrap_or_default().to_owned();
            let commit = match parts.next() {
                Some(sha) if is_commit_sha(sha) => {
                    let branches = parts.next().unwrap_or_default();
                    let subject = parts.next().unwrap_or_default();
                    Some(UpdateGitCommit {
                        sha: sha.to_owned(),
                        branches: branches
                            .strip_prefix("HEAD -> ")
                            .unwrap_or(branches)
                            .to_owned(),
                        subject: subject.chars().filter(|ch| !ch.is_control()).collect(),
                    })
                }
                Some(_) => return Err(UpdateError::new("Git returned an invalid graph commit")),
                None => None,
            };
            Ok(UpdateGitRow { graph, commit })
        })
        .collect()
}

fn parse_commits(output: &str) -> Result<Vec<UpdateCommit>, UpdateError> {
    let mut parts = output.split('\0');
    let mut commits = Vec::new();
    while let Some(sha) = parts.next() {
        if sha.is_empty() {
            break;
        }
        let message = parts
            .next()
            .ok_or_else(|| UpdateError::new("Git returned an incomplete commit"))?;
        if !is_commit_sha(sha) {
            return Err(UpdateError::new("Git returned an invalid commit SHA"));
        }
        commits.push(UpdateCommit {
            sha: sha.to_owned(),
            message: message.trim_end().to_owned(),
        });
    }
    Ok(commits)
}

struct Git {
    work: PathBuf,
    deadline: Instant,
    cancel: UpdateCancellation,
}

impl Git {
    fn repo(&self, args: &[&str]) -> Result<String, UpdateError> {
        let mut command = vec!["--git-dir=history"];
        command.extend_from_slice(args);
        self.run(&command)
    }

    fn run(&self, args: &[&str]) -> Result<String, UpdateError> {
        if Instant::now() >= self.deadline {
            return Err(UpdateError::new("Git update check timed out"));
        }
        self.cancel.check()?;
        let spec = ProcessSpec::new("git")
            .args([
                "-c",
                "credential.helper=",
                "-c",
                "http.lowSpeedLimit=1",
                "-c",
                "http.lowSpeedTime=20",
            ])
            .args(args.iter().copied())
            .current_dir(&self.work)
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_NO_LAZY_FETCH", "1")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE");
        let result = platform::native_platform().spawn_wait_cancellable(&spec, &|| {
            self.cancel.is_cancelled() || Instant::now() >= self.deadline
        });
        self.cancel.check()?;
        if Instant::now() >= self.deadline {
            return Err(UpdateError::new("Git update check timed out"));
        }
        let output = result.map_err(|error| {
            UpdateError::new(format!("could not read history with Git: {error}"))
        })?;
        super::checked_exit(output, "git").map(|exit| {
            exit.stdout
                .utf8_lossy()
                .trim_end_matches(['\r', '\n'])
                .to_owned()
        })
    }
}

#[cfg(test)]
#[path = "../../tests/unit/update_git/tests.rs"]
mod tests;
