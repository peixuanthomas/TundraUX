use super::*;

fn release() -> Release {
    let base = format!("https://github.com/{GITHUB_OWNER}/{GITHUB_REPO}/releases/download/v2.0.1/");
    Release {
        tag_name: "v2.0.1".into(),
        draft: false,
        prerelease: false,
        body: None,
        assets: ["tundraux-2.0.1-linux-x86_64.tar.gz", "SHA256SUMS"]
            .into_iter()
            .map(|name| Asset {
                name: name.into(),
                browser_download_url: format!("{base}{name}"),
                state: "uploaded".into(),
            })
            .collect(),
    }
}

#[test]
fn release_selects_published_linux_archive_and_checksum() {
    let mut value = release();
    assert_eq!(select_payload(&value).unwrap().version, "2.0.1");
    value.prerelease = true;
    assert!(select_payload(&value).is_err());
    value.prerelease = false;
    value.assets.pop();
    assert!(select_payload(&value).is_err());
    let mut value = release();
    value.assets[0].browser_download_url = "https://example.com/payload".into();
    assert!(select_payload(&value).is_err());
}

#[test]
fn release_version_and_commit_both_matter() {
    let identity = BuildIdentity {
        package_version: "2.0.1".into(),
        commit_sha: Some("a".repeat(40)),
        dirty: false,
    };
    let local = Version::new(2, 0, 1);
    assert_eq!(
        release_relation(&identity, &local, &local, &"a".repeat(40)),
        UpdateRelation::Identical
    );
    assert_eq!(
        release_relation(&identity, &local, &Version::new(2, 0, 2), &"b".repeat(40)),
        UpdateRelation::Behind { remote_ahead: 1 }
    );
    assert_eq!(
        release_relation(&identity, &local, &Version::new(2, 0, 0), &"b".repeat(40)),
        UpdateRelation::Ahead { local_ahead: 1 }
    );
    assert_eq!(
        release_relation(&identity, &local, &local, &"b".repeat(40)),
        UpdateRelation::Unknown
    );
}

#[test]
fn checksums_reject_wrong_file_duplicate_and_corrupt_download() {
    let hash = format!("{:x}", Sha256::digest(b"archive"));
    assert_eq!(
        checksum_for(&format!("{hash}  file.tar.gz\n"), "file.tar.gz").unwrap(),
        hash
    );
    assert!(checksum_for(&format!("{hash}  other.tar.gz\n"), "file.tar.gz").is_err());
    assert!(
        checksum_for(
            &format!("{hash}  file.tar.gz\n{hash} *file.tar.gz\n"),
            "file.tar.gz"
        )
        .is_err()
    );
    let mut output = Vec::new();
    download_archive(
        &b"archive"[..],
        &mut output,
        Some(7),
        &hash,
        &mut |_| {},
        &UpdateCancellation::default(),
    )
    .unwrap();
    assert_eq!(output, b"archive");
    assert!(
        download_archive(
            &b"corrupt"[..],
            Vec::new(),
            None,
            &hash,
            &mut |_| {},
            &UpdateCancellation::default()
        )
        .is_err()
    );
}

fn archive(files: &[(&str, tar::EntryType, &[u8])]) -> Vec<u8> {
    let encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    let mut tar = tar::Builder::new(encoder);
    for (name, kind, contents) in files {
        let mut header = tar::Header::new_gnu();
        header.set_entry_type(*kind);
        header.set_mode(0o777);
        let contents = if kind.is_file() { *contents } else { &[] };
        if kind.is_symlink() || kind.is_hard_link() {
            header.set_link_name("../../outside").unwrap();
        }
        header.set_size(contents.len() as u64);
        header.set_cksum();
        tar.append_data(&mut header, name, contents).unwrap();
    }
    tar.into_inner().unwrap().finish().unwrap()
}

fn portable_payload() -> Vec<(&'static str, tar::EntryType, &'static [u8])> {
    let regular = tar::EntryType::Regular;
    vec![
        ("portable/tundra-shell", regular, b"fixture"),
        ("portable/tundra-cli", regular, b"fixture"),
        ("portable/vim/vim", regular, b"vim executable"),
        ("portable/vim/LICENSE", regular, b"Vim License"),
        (
            "portable/vim/runtime/defaults.vim",
            regular,
            b"set compatible",
        ),
        ("portable/vim/runtime/doc/help.txt", regular, b"Vim help"),
        (
            "portable/vim/runtime/syntax",
            tar::EntryType::Directory,
            b"",
        ),
        (
            "portable/vim/runtime/syntax/rust.vim",
            regular,
            b"syntax keyword",
        ),
        (
            "portable/vim/UPSTREAM.json",
            regular,
            super::super::vim::UPSTREAM_MANIFEST.as_bytes(),
        ),
        (
            "portable/assets/user.txt",
            regular,
            b"must not overwrite assets",
        ),
    ]
}

#[test]
fn extracts_binaries_and_complete_vim_runtime_without_replacing_assets() {
    let root = super::super::tests::update_test_root("release-complete-runtime");
    fs::create_dir_all(&root).unwrap();
    extract_executables(
        &archive(&portable_payload())[..],
        &root,
        &UpdateCancellation::default(),
    )
    .unwrap();
    assert!(!root.join("assets").exists());
    assert_eq!(fs::read(root.join(SHELL_FILE)).unwrap(), b"fixture");
    assert_eq!(
        fs::read(root.join("vim/runtime/syntax/rust.vim")).unwrap(),
        b"syntax keyword"
    );
    for (path, mode) in [
        (SHELL_FILE, 0o755),
        (CLI_FILE, 0o755),
        ("vim/vim", 0o755),
        ("vim/runtime/doc/help.txt", 0o644),
        ("vim/runtime/syntax", 0o755),
    ] {
        assert_eq!(
            fs::metadata(root.join(path)).unwrap().permissions().mode() & 0o777,
            mode,
            "{path}"
        );
    }
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn verified_release_is_staged_once_and_only_published_after_both_probes_pass() {
    use std::os::unix::fs::MetadataExt;

    let install = tempfile::tempdir().unwrap();
    let install = install.path();
    fs::write(install.join(SHELL_FILE), b"old shell").unwrap();
    fs::write(install.join(CLI_FILE), b"old cli").unwrap();
    fs::write(
        install.join("tundra-installation.json"),
        include_bytes!("../../../../packaging/linux/tundra-installation.json"),
    )
    .unwrap();
    let probe =
        format!("#!/bin/sh\necho protocol={UPDATE_PROTOCOL_VERSION}\necho commit=fixture\n");
    let cancel = UpdateCancellation::default();
    let mut files = portable_payload();
    for invalid in [0, 1] {
        files[0].2 = probe.as_bytes();
        files[1].2 = probe.as_bytes();
        files[invalid].2 = b"#!/bin/sh\nexit 1\n";
        assert!(stage_verified_archive(&archive(&files)[..], install, "fixture", &cancel).is_err());
        assert_eq!(
            install.join(".tundra-update").read_dir().unwrap().count(),
            0
        );
    }
    files[1].2 = probe.as_bytes();
    let staged = stage_verified_archive(&archive(&files)[..], install, "fixture", &cancel).unwrap();
    let manifest = load_manifest(&staged.manifest_path).unwrap();
    assert_eq!(manifest.state, TransactionState::Prepared);
    let new = manifest.transaction_dir.join("new");
    assert_eq!(
        fs::read(new.join("vim/runtime/syntax/rust.vim")).unwrap(),
        b"syntax keyword"
    );
    assert_eq!(
        fs::metadata(new.join(CLI_FILE)).unwrap().ino(),
        fs::metadata(manifest.transaction_dir.join(HELPER_FILE))
            .unwrap()
            .ino()
    );
    assert_eq!(fs::read(install.join(SHELL_FILE)).unwrap(), b"old shell");
    assert!(!install.join("vim").exists());
}

#[test]
fn rejects_incomplete_duplicated_linked_or_mixed_root_release_payloads() {
    let regular = tar::EntryType::Regular;
    let valid = portable_payload();
    let mut duplicate = valid.clone();
    duplicate.push(("portable/vim/runtime/defaults.vim", regular, b"duplicate"));
    let mut duplicate_directory = valid.clone();
    duplicate_directory.push((
        "portable/vim/runtime/syntax",
        tar::EntryType::Directory,
        b"",
    ));
    let mut linked_vim = valid.clone();
    linked_vim[2].1 = tar::EntryType::Symlink;
    let mut hard_linked_runtime = valid.clone();
    hard_linked_runtime[5].1 = tar::EntryType::Link;
    let mut wrong_vim_root = valid.clone();
    wrong_vim_root[2].0 = "other/vim/vim";
    let mut wrong_cli_root = valid.clone();
    wrong_cli_root[1].0 = "other/tundra-cli";
    let mut invalid_manifest = valid.clone();
    invalid_manifest[8].2 = b"{}";
    let mut duplicate_shell = valid.clone();
    duplicate_shell.push(("portable/tundra-shell", regular, b"duplicate"));
    let mut linked_shell = valid.clone();
    linked_shell[0].1 = tar::EntryType::Symlink;
    for files in [
        valid
            .iter()
            .copied()
            .filter(|(name, _, _)| !name.starts_with("portable/vim/"))
            .collect(),
        valid
            .iter()
            .copied()
            .filter(|(name, _, _)| *name != "portable/vim/runtime/defaults.vim")
            .collect(),
        valid
            .iter()
            .copied()
            .filter(|(name, _, _)| *name != "portable/tundra-shell")
            .collect(),
        duplicate,
        duplicate_directory,
        duplicate_shell,
        linked_shell,
        linked_vim,
        hard_linked_runtime,
        wrong_vim_root,
        wrong_cli_root,
        invalid_manifest,
    ] {
        let root = super::super::tests::update_test_root("release-archive");
        fs::create_dir_all(&root).unwrap();
        assert!(
            extract_executables(&archive(&files)[..], &root, &UpdateCancellation::default())
                .is_err()
        );
        assert!(!root.join("assets").exists());
        fs::remove_dir_all(root).unwrap();
    }
}
