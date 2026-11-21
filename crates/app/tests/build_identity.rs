#[path = "../build_support.rs"]
mod build_support;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

struct TempRepository(PathBuf);

impl Drop for TempRepository {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn git(repository: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(repository)
        .status()
        .expect("git must be available for the build identity regression test");
    assert!(status.success(), "git command failed: {args:?}");
}

fn committed_repository() -> TempRepository {
    let root = std::env::temp_dir().join(format!(
        "tundra-build-identity-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir_all(&root).unwrap();
    let repository = TempRepository(fs::canonicalize(root).unwrap());
    git(&repository.0, &["init", "--quiet", "--template="]);
    fs::write(repository.0.join("tracked.txt"), b"original\n").unwrap();
    git(&repository.0, &["add", "tracked.txt"]);
    git(
        &repository.0,
        &[
            "-c",
            "user.name=Codex Test",
            "-c",
            "user.email=codex-test@local",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "-m",
            "fixture",
        ],
    );
    repository
}

fn watches(identity: &build_support::BuildIdentity, path: &Path) -> bool {
    let path = fs::canonicalize(path).unwrap();
    identity
        .inputs
        .iter()
        .any(|input| fs::canonicalize(input).is_ok_and(|input| input == path))
}

#[test]
fn build_identity_tracks_the_checkout_and_supports_source_archives() {
    let repository = committed_repository();
    let tracked = repository.0.join("tracked.txt");
    let initial = build_support::build_identity(&repository.0, None);
    assert!(initial.commit.is_some());
    assert!(!initial.dirty);
    assert!(watches(&initial, &repository.0.join(".git/index")));

    let archive = repository.0.join("archive");
    fs::create_dir(&archive).unwrap();
    let archived = build_support::build_identity(&archive, None);
    assert!(archived.commit.is_none());
    assert!(!archived.dirty);
    assert!(archived.inputs.is_empty());

    fs::write(&tracked, b"changed\n").unwrap();
    assert!(build_support::build_identity(&repository.0, None).dirty);
    let explicit = build_support::build_identity(&repository.0, Some("override".into()));
    assert_eq!(explicit.commit.as_deref(), Some("override"));
    assert!(!explicit.dirty);
    assert!(explicit.inputs.is_empty());

    fs::remove_file(&tracked).unwrap();
    let deleted = build_support::build_identity(&repository.0, None);
    assert!(deleted.dirty);
    assert!(deleted.inputs.contains(&tracked));

    fs::write(&tracked, b"original\n").unwrap();
    fs::write(repository.0.join("untracked.txt"), b"untracked\n").unwrap();
    assert!(!build_support::build_identity(&repository.0, None).dirty);

    fs::write(&tracked, b"staged\n").unwrap();
    git(&repository.0, &["add", "tracked.txt"]);
    fs::write(&tracked, b"original\n").unwrap();
    assert!(build_support::build_identity(&repository.0, None).dirty);
    git(
        &repository.0,
        &["reset", "--quiet", "HEAD", "--", "tracked.txt"],
    );
    assert!(!build_support::build_identity(&repository.0, None).dirty);

    let linked = repository.0.join("linked");
    git(
        &repository.0,
        &["worktree", "add", "--quiet", "-b", "linked-test", "linked"],
    );
    git(&repository.0, &["pack-refs", "--all", "--prune"]);
    let identity = build_support::build_identity(&linked, None);
    assert_eq!(identity.commit, initial.commit);
    assert!(!identity.dirty);
    let git_dir = repository.0.join(".git");
    assert!(watches(&identity, &git_dir.join("worktrees/linked/HEAD")));
    assert!(watches(&identity, &git_dir.join("worktrees/linked/index")));
    assert!(watches(&identity, &git_dir.join("packed-refs")));
    let loose_ref = git_dir.join("refs/heads/linked-test");
    assert!(!loose_ref.exists());
    assert!(identity.inputs.iter().all(|path| path.exists()));
    assert!(identity.inputs.iter().any(|path| path.is_dir()
        && fs::canonicalize(path).is_ok_and(|path| loose_ref.starts_with(path))));
}
