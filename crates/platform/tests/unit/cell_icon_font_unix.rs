use super::*;

const FONT: &[u8] = b"bundled test font";

fn fixture() -> (tempfile::TempDir, FontPaths) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let paths = FontPaths::new(root.join("fonts"), root.join("owner"));
    (temp, paths)
}

#[test]
fn owned_font_survives_a_retry_and_can_be_removed() {
    let (_temp, paths) = fixture();
    assert_eq!(
        paths.install(FONT, || Ok(Vec::new()), |_| Ok(())).unwrap(),
        CellIconFontInstall::Installed
    );
    assert_eq!(
        paths.install(FONT, || Ok(Vec::new()), |_| Ok(())).unwrap(),
        CellIconFontInstall::Available
    );
    assert_eq!(
        paths.uninstall(|_| Ok(())).unwrap(),
        CellIconFontRemoval::Removed
    );
    assert!(!paths.font.exists());
    assert!(!paths.record.exists());
    assert_eq!(
        paths.install(FONT, || Ok(Vec::new()), |_| Ok(())).unwrap(),
        CellIconFontInstall::Installed
    );
}

#[test]
fn a_manually_installed_font_is_reused_and_kept() {
    let (temp, paths) = fixture();
    let manual = temp.path().join("my-font.ttf");
    fs::write(&manual, FONT).unwrap();
    let inventory = || Ok(vec![manual.clone()]);

    assert_eq!(
        paths.install(FONT, inventory, |_| Ok(())).unwrap(),
        CellIconFontInstall::Available
    );
    assert!(!paths.record.exists());
    assert_eq!(
        paths.uninstall(|_| Ok(())).unwrap(),
        CellIconFontRemoval::Missing
    );
    assert_eq!(fs::read(&manual).unwrap(), FONT);

    fs::write(&manual, b"different font version").unwrap();
    assert_eq!(
        paths.install(FONT, inventory, |_| Ok(())).unwrap(),
        CellIconFontInstall::Conflict
    );
    assert_eq!(fs::read(&manual).unwrap(), b"different font version");
}

#[test]
fn an_identical_replacement_is_preserved() {
    let (temp, paths) = fixture();
    paths.install(FONT, || Ok(Vec::new()), |_| Ok(())).unwrap();
    fs::rename(&paths.font, temp.path().join("original.ttf")).unwrap();
    fs::write(&paths.font, FONT).unwrap();

    assert_eq!(
        paths.uninstall(|_| Ok(())).unwrap(),
        CellIconFontRemoval::Preserved
    );
    assert_eq!(fs::read(&paths.font).unwrap(), FONT);
}

#[test]
fn install_can_resume_after_the_ownership_record_is_written() {
    let (_temp, paths) = fixture();
    paths.install(FONT, || Ok(Vec::new()), |_| Ok(())).unwrap();
    let mut record = paths.ownership().unwrap().unwrap();
    record.phase = FontPhase::Pending;
    paths.save_ownership(&record).unwrap();
    fs::rename(&paths.font, paths.font.with_file_name(record.stage_file)).unwrap();

    assert_eq!(
        paths.install(FONT, || Ok(Vec::new()), |_| Ok(())).unwrap(),
        CellIconFontInstall::Installed
    );
    assert_eq!(fs::read(&paths.font).unwrap(), FONT);
}

#[test]
fn install_retries_a_failed_cache_refresh() {
    let (_temp, paths) = fixture();
    assert!(
        paths
            .install(
                FONT,
                || Ok(Vec::new()),
                |_| Err(io::Error::other("test failure"))
            )
            .is_err()
    );
    assert!(paths.font.exists());
    assert!(paths.ownership().unwrap().unwrap().phase == FontPhase::Pending);

    assert_eq!(
        paths
            .install(
                FONT,
                || Ok(Vec::new()),
                |fonts| {
                    assert_eq!(fonts.join(FILE_NAME), paths.font);
                    Ok(())
                }
            )
            .unwrap(),
        CellIconFontInstall::Installed
    );
    assert_eq!(
        paths
            .install(
                FONT,
                || Ok(Vec::new()),
                |_| panic!("cache is already refreshed")
            )
            .unwrap(),
        CellIconFontInstall::Available
    );
}

#[test]
fn uninstall_restores_a_replacement_moved_into_the_staging_path() {
    let (temp, paths) = fixture();
    paths.install(FONT, || Ok(Vec::new()), |_| Ok(())).unwrap();
    let mut record = paths.ownership().unwrap().unwrap();
    record.phase = FontPhase::Removing;
    paths.save_ownership(&record).unwrap();
    let stage = paths.font.with_file_name(record.stage_file);
    let replaced = b"external replacement";
    fs::rename(&paths.font, temp.path().join("original.ttf")).unwrap();
    fs::write(&paths.font, replaced).unwrap();
    fs::rename(&paths.font, &stage).unwrap();

    assert_eq!(
        paths.uninstall(|_| Ok(())).unwrap(),
        CellIconFontRemoval::Preserved
    );
    assert_eq!(fs::read(&paths.font).unwrap(), replaced);
    assert!(!stage.exists());
}

#[test]
fn a_failed_cache_refresh_keeps_the_ownership_record_for_reset() {
    let (_temp, paths) = fixture();
    paths.install(FONT, || Ok(Vec::new()), |_| Ok(())).unwrap();
    assert!(
        paths
            .uninstall(|_| Err(io::Error::other("test failure")))
            .is_err()
    );
    assert!(paths.record.exists());
    assert_eq!(
        paths.uninstall(|_| Ok(())).unwrap(),
        CellIconFontRemoval::Removed
    );
    assert!(!paths.record.exists());
}
