use super::*;

#[test]
fn update_git_reads_history_without_a_checkout_or_api() {
    let root = super::super::tests::update_test_root("git-history");
    fs::create_dir_all(&root).unwrap();
    let git = Git {
        work: root.clone(),
        cancel: UpdateCancellation::default(),
        deadline: Instant::now() + Duration::from_secs(60),
    };
    git.run(&["init", "--bare", "--initial-branch=master", "history"])
        .unwrap();
    let tree = git.repo(&["mktree"]).unwrap();
    let commit = |message: &str, parents: &[&str]| {
        let mut args = vec![
            "-c",
            "user.name=Codex",
            "-c",
            "user.email=codex@local",
            "commit-tree",
            &tree,
            "-m",
            message,
        ];
        for parent in parents {
            args.extend(["-p", parent]);
        }
        git.repo(&args).unwrap()
    };
    let first = commit("first", &[]);
    let second = commit("second\n\nDetailed body", &[&first]);
    let joined = commit("joined", &[&first]);
    let head = commit("merge", &[&second, &joined]);
    let unmerged = commit("unmerged", &[&second]);
    git.repo(&["update-ref", "refs/heads/master", &head])
        .unwrap();
    git.repo(&["update-ref", "refs/heads/feature/unmerged", &unmerged])
        .unwrap();
    #[cfg(target_os = "linux")]
    {
        // Beta must still follow master if GitHub's default branch changes.
        git.repo(&["update-ref", "refs/heads/other", &second])
            .unwrap();
        git.repo(&["symbolic-ref", "HEAD", "refs/heads/other"])
            .unwrap();
    }
    let remote = reqwest::Url::from_directory_path(root.join("history"))
        .expect("absolute repository path must convert to a file URL");
    for (index, (local, expected, count)) in [
        (
            Some(first.clone()),
            UpdateRelation::Behind { remote_ahead: 3 },
            3,
        ),
        (Some(head.clone()), UpdateRelation::Identical, 0),
        (
            Some(unmerged.clone()),
            UpdateRelation::Diverged {
                remote_ahead: 2,
                local_ahead: 1,
            },
            2,
        ),
        (Some("a".repeat(40)), UpdateRelation::Unknown, 4),
        (None, UpdateRelation::Unknown, 4),
        (Some("--invalid".into()), UpdateRelation::Unknown, 4),
    ]
    .into_iter()
    .enumerate()
    {
        let work = root.join(index.to_string());
        fs::create_dir(&work).unwrap();
        let result = check_in(
            &BuildIdentity {
                package_version: "0.1.1".into(),
                commit_sha: local,
                dirty: false,
            },
            remote.as_str(),
            &work,
            &UpdateCancellation::default(),
        )
        .unwrap();
        assert_eq!(result.default_branch, "master");
        assert_eq!(result.head_sha, head);
        assert_eq!(result.relation, expected);
        assert_eq!(result.commits.len(), count);
        if index == 0 {
            assert!(
                result
                    .commits
                    .iter()
                    .any(|commit| commit.message == "second\n\nDetailed body")
            );
            assert_eq!(result.commits.last().unwrap().sha, head);
        }
        if index == 1 {
            let rows = result.git_rows();
            assert_eq!(rows.iter().filter(|row| row.commit.is_some()).count(), 5);
            assert!(
                rows.iter()
                    .any(|row| row.commit.is_none() && row.graph.contains(['/', '\\']))
            );
            assert!(
                rows.iter()
                    .filter_map(|row| row.commit.as_ref())
                    .any(|commit| {
                        commit.sha == unmerged && commit.branches == "feature/unmerged"
                    })
            );
            for target in [&first, &unmerged] {
                let selected = result.for_selected_build(target).unwrap();
                assert!(
                    super::super::source_archive_url(&selected.head_sha)
                        .unwrap()
                        .ends_with(target)
                );
                assert_eq!(result.head_sha, head);
            }
            assert!(result.for_selected_build(&"f".repeat(40)).is_none());
        }
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn update_git_expired_deadline_does_not_launch_a_command() {
    let git = Git {
        work: PathBuf::new(),
        cancel: UpdateCancellation::default(),
        deadline: Instant::now(),
    };
    assert!(
        git.run(&["--version"])
            .unwrap_err()
            .to_string()
            .contains("timed out")
    );
}

#[test]
fn update_git_fallback_cancels_its_shell_and_descendants() {
    let root = super::super::tests::update_test_root("git-cancel");
    fs::create_dir_all(&root).unwrap();
    let cancel = UpdateCancellation::default();
    let worker_cancel = cancel.clone();
    let git = Git {
        work: root.clone(),
        deadline: Instant::now() + Duration::from_secs(60),
        cancel,
    };
    let request = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(150));
        worker_cancel.cancel();
    });
    let started = Instant::now();
    let result = git.run(&["-c", "alias.wait-update=!sleep 30", "wait-update"]);
    request.join().unwrap();
    assert!(result.unwrap_err().is_cancelled());
    assert!(started.elapsed() < Duration::from_secs(3));
    fs::remove_dir_all(root).unwrap();
}
