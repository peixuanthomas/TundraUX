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
        let file_name = Path::new(&ownership.stage_file);
        if !matches!(
            file_name.components().collect::<Vec<_>>().as_slice(),
            [Component::Normal(_)]
        ) || !ownership
            .stage_file
            .as_encoded_bytes()
            .starts_with(STAGE_PREFIX.as_bytes())
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
        if let Some(current) = &current
            && (current.sha256 != expected_hash
                || record
                    .as_ref()
                    .is_some_and(|record| record.fingerprint != *current))
        {
            return Ok(Conflict);
        }

        let mut available = current.is_some();
        for installed in inventory()? {
            // A manually installed font can be exposed through a symlink.
            let installed = match fs::canonicalize(installed) {
                Ok(path) => path,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error),
            };
            match fingerprint(&installed)? {
                Some(installed) if installed.sha256 == expected_hash => available = true,
                Some(_) => return Ok(Conflict),
                None => {}
            }
        }
        if available {
            if current.is_some()
                && let Some(record) = record
                && record.phase != FontPhase::Ready
            {
                return self.finish_install(fonts, record, refresh);
            }
            return Ok(Available);
        }

        if let Some(record) = record {
            let stage = fonts.join(&record.stage_file);
            match fingerprint(&stage) {
                Ok(Some(fingerprint)) if fingerprint == record.fingerprint => {
                    if fingerprint.sha256 != expected_hash {
                        return Ok(Conflict);
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
        }

        fs::create_dir_all(fonts)?;
        let mut stage = tempfile::Builder::new()
            .prefix(STAGE_PREFIX)
            .tempfile_in(fonts)?;
        stage.write_all(font)?;
        stage.as_file().sync_all()?;
        let fingerprint = fingerprint(stage.path())?.expect("just staged font");
        let ownership = Ownership {
            stage_file: stage
                .path()
                .file_name()
                .expect("staged filename")
                .to_owned(),
            fingerprint,
            phase: FontPhase::Pending,
        };
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
