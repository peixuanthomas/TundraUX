use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{CellIconFontInstall, CellIconFontRemoval};
#[cfg(target_os = "linux")]
use crate::ManagedCommand;
use crate::{AppPaths, atomic_write_document, rename_path_no_replace, validate_no_follow_path};

#[cfg(target_os = "macos")]
#[path = "macos.rs"]
mod macos;

const FAMILY: &str = "Tundra Cell Atlas";
const FILE_NAME: &str = "TundraCellAtlas.ttf";
const STAGE_PREFIX: &str = ".TundraCellAtlas-";

struct FontPaths {
    font: PathBuf,
    record: PathBuf,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Fingerprint {
    device: u64,
    inode: u64,
    modified_seconds: i64,
    modified_nanos: i64,
    sha256: [u8; 32],
}

#[derive(Serialize, Deserialize)]
struct Ownership {
    stage_file: OsString,
    fingerprint: Fingerprint,
    #[serde(default)]
    phase: FontPhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    upgrade: Option<StagedFont>,
}

#[derive(Serialize, Deserialize)]
struct StagedFont {
    stage_file: OsString,
    fingerprint: Fingerprint,
}

impl StagedFont {
    fn into_ownership(self, phase: FontPhase) -> Ownership {
        Ownership {
            stage_file: self.stage_file,
            fingerprint: self.fingerprint,
            phase,
            upgrade: None,
        }
    }
}

#[derive(Default, PartialEq, Eq, Serialize, Deserialize)]
enum FontPhase {
    #[default]
    Pending,
    Ready,
    Removing,
}

impl FontPaths {
    fn native(paths: &AppPaths) -> Self {
        let app_directory = paths.data_path().parent().expect("native app directory");
        #[cfg(target_os = "linux")]
        let fonts = app_directory.with_file_name("fonts");
        #[cfg(target_os = "macos")]
        let fonts = app_directory
            .parent()
            .and_then(Path::parent)
            .expect("native Library directory")
            .join("Fonts");
        Self::new(fonts, app_directory.join("cell-icon-font"))
    }

    fn new(fonts: PathBuf, owner: PathBuf) -> Self {
        Self {
            font: fonts.join(FILE_NAME),
            record: owner.join("ownership.json"),
        }
    }

    fn lock(&self) -> io::Result<File> {
        let owner = self.record.parent().expect("ownership directory");
        validate_no_follow_path(owner, false).map_err(io::Error::other)?;
        fs::create_dir_all(owner)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(owner.join("install.lock"))?;
        if !lock.metadata()?.is_file() {
            return Err(io::Error::other("invalid cell icon font lock"));
        }
        lock.lock()?;
        Ok(lock)
    }

    fn ownership(&self) -> io::Result<Option<Ownership>> {
        let bytes = match read_regular_file(&self.record)? {
            Some(file) => {
                use std::io::Read;
                let mut bytes = Vec::new();
                file.take(4096).read_to_end(&mut bytes)?;
                bytes
            }
            None => return Ok(None),
        };
        let ownership: Ownership = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
        if !valid_stage_name(&ownership.stage_file)
            || ownership.upgrade.as_ref().is_some_and(|upgrade| {
                !valid_stage_name(&upgrade.stage_file) || upgrade.stage_file == ownership.stage_file
            })
        {
            return Err(io::Error::other("invalid cell icon font ownership file"));
        }
        Ok(Some(ownership))
    }

    fn save_ownership(&self, record: &Ownership) -> io::Result<()> {
        atomic_write_document(&self.record, &serde_json::to_vec(record)?)
            .map_err(io::Error::other)?;
        Ok(())
    }

    fn finish_install(
        &self,
        fonts: &Path,
        mut record: Ownership,
        refresh: impl FnOnce(&Path) -> io::Result<()>,
    ) -> io::Result<CellIconFontInstall> {
        File::open(fonts)?.sync_all()?;
        refresh(fonts)?;
        record.phase = FontPhase::Ready;
        self.save_ownership(&record)?;
        Ok(CellIconFontInstall::Installed)
    }

    fn restore_staged_file(&self, stage: &Path, phase: FontPhase) -> io::Result<()> {
        if phase == FontPhase::Removing {
            rename_path_no_replace(stage, &self.font).map_err(io::Error::other)?;
        }
        Ok(())
    }

    fn prepare_upgrade(&self, fonts: &Path, record: &mut Ownership, font: &[u8]) -> io::Result<()> {
        let (file, staged) = stage_font(fonts, font)?;
        record.upgrade = Some(staged);
        self.save_ownership(record)?;
        file.keep().map_err(|error| error.error)?;
        Ok(())
    }

    fn upgrade(
        &self,
        fonts: &Path,
        mut record: Ownership,
        current: Option<&Fingerprint>,
        font: &[u8],
        refresh: impl FnOnce(&Path) -> io::Result<()>,
    ) -> io::Result<CellIconFontInstall> {
        use CellIconFontInstall::*;

        let backup = fonts.join(&record.stage_file);
        let backup_state = owned_file(&backup, &record.fingerprint)?;
        if backup_state == OwnedFile::Changed {
            if current.is_none() {
                self.restore_staged_file(&backup, record.phase)?;
            }
            return Ok(Conflict);
        }
        let expected_hash: [u8; 32] = Sha256::digest(font).into();
        if record.upgrade.is_none() {
            self.prepare_upgrade(fonts, &mut record, font)?;
        }
        let next = record.upgrade.as_ref().expect("prepared upgrade");
        if next.fingerprint.sha256 != expected_hash {
            return Ok(Conflict);
        }
        if current != Some(&next.fingerprint) {
            match owned_file(&fonts.join(&next.stage_file), &next.fingerprint)? {
                OwnedFile::Changed => return Ok(Conflict),
                OwnedFile::Missing => self.prepare_upgrade(fonts, &mut record, font)?,
                OwnedFile::Matches => {}
            }
            if current.is_some() {
                if backup_state != OwnedFile::Missing {
                    return Ok(Conflict);
                }
                // Record the move before it can capture a replacement written by another program.
                record.phase = FontPhase::Removing;
                self.save_ownership(&record)?;
                rename_path_no_replace(&self.font, &backup).map_err(io::Error::other)?;
                if owned_file(&backup, &record.fingerprint)? != OwnedFile::Matches {
                    self.restore_staged_file(&backup, record.phase)?;
                    return Ok(Conflict);
                }
            }
            let next = record.upgrade.as_ref().expect("prepared upgrade");
            rename_path_no_replace(&fonts.join(&next.stage_file), &self.font)
                .map_err(io::Error::other)?;
        }
        let next = record.upgrade.expect("prepared upgrade");
        if owned_file(&self.font, &next.fingerprint)? != OwnedFile::Matches {
            return Ok(Conflict);
        }
        File::open(fonts)?.sync_all()?;
        refresh(fonts)?;
        if !remove_owned_file(&backup, &record.fingerprint)? {
            return Ok(Conflict);
        }
        File::open(fonts)?.sync_all()?;
        self.save_ownership(&next.into_ownership(FontPhase::Ready))?;
        Ok(Installed)
    }

    fn install(
        &self,
        font: &[u8],
        inventory: impl FnOnce() -> io::Result<Vec<PathBuf>>,
        refresh: impl FnOnce(&Path) -> io::Result<()>,
    ) -> io::Result<CellIconFontInstall> {
        use CellIconFontInstall::*;

        let _lock = self.lock()?;
        let fonts = self.font.parent().expect("font directory");
        validate_no_follow_path(fonts, false).map_err(io::Error::other)?;
        let expected_hash: [u8; 32] = Sha256::digest(font).into();
        let record = self.ownership()?;
        let current = match fingerprint(&self.font) {
            Err(error) if error.kind() == io::ErrorKind::InvalidInput => return Ok(Conflict),
            result => result?,
        };
        if let Some(current) = &current {
            let matches = match &record {
                Some(record) => {
                    record.fingerprint == *current
                        || record
                            .upgrade
                            .as_ref()
                            .is_some_and(|next| next.fingerprint == *current)
                }
                None => current.sha256 == expected_hash,
            };
            if !matches {
                return Ok(Conflict);
            }
        }

        let current_path = current
            .as_ref()
            .map(|_| fs::canonicalize(&self.font))
            .transpose()?;
        let owned_backup = record
            .as_ref()
            .filter(|record| record.upgrade.is_some())
            .map(|record| (fonts.join(&record.stage_file), &record.fingerprint));
        let mut available = current
            .as_ref()
            .is_some_and(|file| file.sha256 == expected_hash);
        for installed in inventory()? {
            // A manually installed font can be exposed through a symlink.
            let installed = match fs::canonicalize(installed) {
                Ok(path) => path,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            if current_path.as_ref() == Some(&installed) {
                continue;
            }
            if let Some((backup, expected)) = &owned_backup
                && &installed == backup
                && owned_file(backup, expected)? == OwnedFile::Matches
            {
                continue;
            }
            match fingerprint(&installed)? {
                Some(installed) if installed.sha256 == expected_hash => available = true,
                Some(_) => return Ok(Conflict),
                None => {}
            }
        }
        if let Some(record) = record {
            if record.upgrade.is_some()
                || current
                    .as_ref()
                    .is_some_and(|file| file.sha256 != expected_hash)
            {
                return self.upgrade(fonts, record, current.as_ref(), font, refresh);
            }
            if available {
                if current.is_some() && record.phase != FontPhase::Ready {
                    return self.finish_install(fonts, record, refresh);
                }
                return Ok(Available);
            }
            let stage = fonts.join(&record.stage_file);
            match fingerprint(&stage) {
                Ok(Some(fingerprint)) if fingerprint == record.fingerprint => {
                    if fingerprint.sha256 != expected_hash {
                        return self.upgrade(fonts, record, None, font, refresh);
                    }
                    rename_path_no_replace(&stage, &self.font).map_err(io::Error::other)?;
                    return self.finish_install(fonts, record, refresh);
                }
                Ok(None) => fs::remove_file(&self.record)?,
                Ok(Some(_)) => {
                    self.restore_staged_file(&stage, record.phase)?;
                    return Ok(Conflict);
                }
                Err(error) if error.kind() == io::ErrorKind::InvalidInput => {
                    self.restore_staged_file(&stage, record.phase)?;
                    return Ok(Conflict);
                }
                Err(error) => return Err(error),
            }
        } else if available {
            return Ok(Available);
        }

        let (stage, copy) = stage_font(fonts, font)?;
        let ownership = copy.into_ownership(FontPhase::Pending);
        self.save_ownership(&ownership)?;
        let (_file, stage) = stage.keep().map_err(|error| error.error)?;
        rename_path_no_replace(&stage, &self.font).map_err(io::Error::other)?;
        self.finish_install(fonts, ownership, refresh)
    }

    fn uninstall(
        &self,
        refresh: impl FnOnce(&Path) -> io::Result<()>,
    ) -> io::Result<CellIconFontRemoval> {
        use CellIconFontRemoval::*;

        let _lock = self.lock()?;
        let Some(mut record) = self.ownership()? else {
            return Ok(Missing);
        };
        let fonts = self.font.parent().expect("font directory");
        validate_no_follow_path(fonts, false).map_err(io::Error::other)?;
        if let Some(next) = record.upgrade.take() {
            match owned_file(&self.font, &next.fingerprint)? {
                OwnedFile::Matches => {
                    if !remove_owned_file(&fonts.join(&record.stage_file), &record.fingerprint)? {
                        return Ok(Preserved);
                    }
                    record = next.into_ownership(FontPhase::Pending);
                }
                OwnedFile::Changed
                    if owned_file(&self.font, &record.fingerprint)? != OwnedFile::Matches =>
                {
                    return Ok(Preserved);
                }
                _ => {
                    if !remove_owned_file(&fonts.join(&next.stage_file), &next.fingerprint)? {
                        return Ok(Preserved);
                    }
                }
            }
            self.save_ownership(&record)?;
        }
        let stage = fonts.join(&record.stage_file);
        match fingerprint(&self.font) {
            Ok(Some(current)) if current == record.fingerprint => {
                match fs::symlink_metadata(&stage) {
                    Ok(_) => return Ok(Preserved),
                    Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                    Err(error) => return Err(error),
                }
                // Remove through the staging name so an external replacement is rechecked.
                record.phase = FontPhase::Removing;
                self.save_ownership(&record)?;
                rename_path_no_replace(&self.font, &stage).map_err(io::Error::other)?;
            }
            Ok(None) => {}
            Ok(Some(_)) => return Ok(Preserved),
            Err(error) if error.kind() == io::ErrorKind::InvalidInput => return Ok(Preserved),
            Err(error) => return Err(error),
        }
        match fingerprint(&stage) {
            Ok(Some(current)) if current == record.fingerprint => fs::remove_file(&stage)?,
            Ok(None) => {}
            Ok(Some(_)) => {
                self.restore_staged_file(&stage, record.phase)?;
                return Ok(Preserved);
            }
            Err(error) if error.kind() == io::ErrorKind::InvalidInput => {
                self.restore_staged_file(&stage, record.phase)?;
                return Ok(Preserved);
            }
            Err(error) => return Err(error),
        }
        if fonts.is_dir() {
            File::open(fonts)?.sync_all()?;
            refresh(fonts)?;
        }
        fs::remove_file(&self.record)?;
        Ok(Removed)
    }
}

fn valid_stage_name(file: &std::ffi::OsStr) -> bool {
    let mut components = Path::new(file).components();
    matches!(components.next(), Some(Component::Normal(_)))
        && components.next().is_none()
        && file.as_encoded_bytes().starts_with(STAGE_PREFIX.as_bytes())
}

fn stage_font(fonts: &Path, font: &[u8]) -> io::Result<(tempfile::NamedTempFile, StagedFont)> {
    fs::create_dir_all(fonts)?;
    let mut file = tempfile::Builder::new()
        .prefix(STAGE_PREFIX)
        .tempfile_in(fonts)?;
    file.write_all(font)?;
    file.as_file().sync_all()?;
    let staged = StagedFont {
        stage_file: file.path().file_name().expect("staged filename").to_owned(),
        fingerprint: fingerprint(file.path())?.expect("just staged font"),
    };
    Ok((file, staged))
}

#[derive(PartialEq, Eq)]
enum OwnedFile {
    Missing,
    Matches,
    Changed,
}

fn owned_file(path: &Path, expected: &Fingerprint) -> io::Result<OwnedFile> {
    match fingerprint(path) {
        Ok(Some(actual)) if actual == *expected => Ok(OwnedFile::Matches),
        Ok(None) => Ok(OwnedFile::Missing),
        Ok(Some(_)) => Ok(OwnedFile::Changed),
        Err(error) if error.kind() == io::ErrorKind::InvalidInput => Ok(OwnedFile::Changed),
        Err(error) => Err(error),
    }
}

fn remove_owned_file(path: &Path, expected: &Fingerprint) -> io::Result<bool> {
    match owned_file(path, expected)? {
        OwnedFile::Matches => fs::remove_file(path)?,
        OwnedFile::Missing => {}
        OwnedFile::Changed => return Ok(false),
    }
    Ok(true)
}

fn read_regular_file(path: &Path) -> io::Result<Option<File>> {
    let invalid = || io::Error::new(io::ErrorKind::InvalidInput, "expected a regular font file");
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
        Ok(metadata) if !metadata.is_file() => return Err(invalid()),
        Ok(_) => {}
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Err(invalid());
    }
    Ok(Some(file))
}

fn fingerprint(path: &Path) -> io::Result<Option<Fingerprint>> {
    let Some(mut file) = read_regular_file(path)? else {
        return Ok(None);
    };
    let mut sha = Sha256::new();
    io::copy(&mut file, &mut sha)?;
    let metadata = file.metadata()?;
    Ok(Some(Fingerprint {
        device: metadata.dev(),
        inode: metadata.ino(),
        modified_seconds: metadata.mtime(),
        modified_nanos: metadata.mtime_nsec(),
        sha256: sha.finalize().into(),
    }))
}

pub(super) fn install(paths: &AppPaths, font: &[u8]) -> io::Result<CellIconFontInstall> {
    FontPaths::native(paths).install(font, inventory, refresh_font_cache)
}

pub(super) fn uninstall(paths: &AppPaths) -> io::Result<CellIconFontRemoval> {
    FontPaths::native(paths).uninstall(refresh_font_cache)
}

#[cfg(target_os = "linux")]
fn inventory() -> io::Result<Vec<PathBuf>> {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;
    use std::process::Command;

    let output = match Command::new("fc-list")
        .args(["--format", "%{file}\n", FAMILY])
        .output_managed()
    {
        Ok(output) if output.status.success() => output,
        Ok(output) => {
            return Err(io::Error::other(
                String::from_utf8_lossy(&output.stderr).into_owned(),
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    Ok(output
        .stdout
        .split(|byte| *byte == b'\n')
        .filter(|path| !path.is_empty())
        .map(|path| PathBuf::from(OsStr::from_bytes(path)))
        .collect())
}

#[cfg(target_os = "macos")]
fn inventory() -> io::Result<Vec<PathBuf>> {
    macos::font_paths(FAMILY)
}

#[cfg(target_os = "linux")]
fn refresh_font_cache(fonts: &Path) -> io::Result<()> {
    use std::process::Command;

    match Command::new("fc-cache")
        .arg("-f")
        .arg(fonts)
        .output_managed()
    {
        Ok(output) if output.status.success() => Ok(()),
        Ok(output) => Err(io::Error::other(
            String::from_utf8_lossy(&output.stderr).into_owned(),
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(target_os = "macos")]
fn refresh_font_cache(_fonts: &Path) -> io::Result<()> {
    // macOS Core Text watches the per-user Library/Fonts directory.
    Ok(())
}

#[cfg(test)]
#[path = "../../tests/unit/cell_icon_font_unix.rs"]
mod tests;
