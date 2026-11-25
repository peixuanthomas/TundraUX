//! Archive codecs for Explorer's staged file tasks.

mod paths;
mod rar;
mod seven_zip;
mod tarball;
mod zip;

use std::ffi::OsString;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use paths::Output;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArchiveFormat {
    Zip,
    SevenZip,
    Rar,
    Tar,
    TarGz,
}

impl ArchiveFormat {
    pub fn from_path(path: &Path) -> Option<Self> {
        archive_suffix(path).and_then(|(format, _)| format)
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Zip => "zip",
            Self::SevenZip => "7z",
            Self::Rar => "rar",
            Self::Tar => "tar",
            Self::TarGz => "tar.gz",
        }
    }

    pub fn supports_password(self) -> bool {
        matches!(self, Self::Zip | Self::SevenZip | Self::Rar)
    }
}

pub fn is_recognized_archive(path: &Path) -> bool {
    archive_suffix(path).is_some()
}

pub fn default_extract_name(path: &Path) -> Option<OsString> {
    let (format, suffix) = archive_suffix(path)?;
    format?;
    if path.file_name()?.len() == suffix.len() {
        return None;
    }
    let mut stem = path.file_stem()?;
    if suffix == ".tar.gz" {
        stem = Path::new(stem).file_stem()?;
    }
    (!stem.is_empty() && stem != "." && stem != "..").then(|| stem.to_os_string())
}

fn archive_suffix(path: &Path) -> Option<(Option<ArchiveFormat>, &'static str)> {
    let name = path.file_name()?.as_encoded_bytes();
    for (format, suffix) in [
        (ArchiveFormat::TarGz, ".tar.gz"),
        (ArchiveFormat::TarGz, ".tgz"),
        (ArchiveFormat::Zip, ".zip"),
        (ArchiveFormat::SevenZip, ".7z"),
        (ArchiveFormat::Rar, ".rar"),
        (ArchiveFormat::Tar, ".tar"),
    ] {
        if name.len() >= suffix.len()
            && name[name.len() - suffix.len()..].eq_ignore_ascii_case(suffix.as_bytes())
        {
            return Some((Some(format), suffix));
        }
    }
    let extension = path.extension()?.to_str()?;
    [
        "7zip", "ace", "ar", "bz", "bz2", "bzip2", "cab", "cpio", "gz", "gzip", "iso", "lha",
        "lzh", "lz", "lz4", "lzma", "tbz", "tbz2", "tlz", "txz", "tzst", "xz", "z", "zipx", "zst",
    ]
    .into_iter()
    .find(|suffix| extension.eq_ignore_ascii_case(suffix))
    .map(|suffix| (None, suffix))
}

#[derive(Debug)]
pub enum ArchiveError {
    Cancelled,
    PasswordRequired,
    InvalidPassword,
    InvalidArchive,
    UnsafePath(PathBuf),
    DuplicatePath(PathBuf),
    UnsupportedEntry(PathBuf),
    UnsupportedEncryption,
    Io(io::Error),
}

impl fmt::Display for ArchiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => f.write_str("archive task cancelled"),
            Self::PasswordRequired => f.write_str("archive password required"),
            Self::InvalidPassword => f.write_str("incorrect archive password"),
            Self::InvalidArchive => f.write_str("invalid archive"),
            Self::UnsafePath(path) => write!(f, "unsafe archive path {}", path.display()),
            Self::DuplicatePath(path) => write!(f, "duplicate archive path {}", path.display()),
            Self::UnsupportedEntry(path) => {
                write!(f, "unsupported archive entry {}", path.display())
            }
            Self::UnsupportedEncryption => f.write_str("archive encryption is unsupported"),
            Self::Io(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for ArchiveError {}

impl From<io::Error> for ArchiveError {
    fn from(error: io::Error) -> Self {
        match error.downcast::<Self>() {
            Ok(error) => error,
            Err(error) => Self::Io(error),
        }
    }
}

/// Writes selected files and directories to a staging file that must not exist.
/// Progress reports the current archive-relative path and cumulative input bytes.
/// Return `false` to cancel. The caller owns cleanup of partial output.
pub fn create(
    format: ArchiveFormat,
    sources: &[PathBuf],
    staging: &Path,
    password: Option<&str>,
    progress: impl FnMut(&Path, u64) -> bool + Send + 'static,
) -> Result<(), ArchiveError> {
    let password = checked_password(format, password)?;
    let progress = Progress::new(progress);
    progress.report(Path::new(""), 0)?;
    // Enumerate before opening the staging file, even if it is inside a selected folder.
    let sources = paths::collect_sources(sources, &progress)?;
    let file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(staging)?;
    let file = match format {
        ArchiveFormat::Zip => zip::create(file, &sources, password, &progress)?,
        ArchiveFormat::SevenZip => seven_zip::create(file, &sources, password, &progress)?,
        ArchiveFormat::Rar => rar::create(file, &sources, password, &progress)?,
        ArchiveFormat::Tar => tarball::create(file, &sources, false, &progress)?,
        ArchiveFormat::TarGz => tarball::create(file, &sources, true, &progress)?,
    };
    file.sync_all()?;
    progress.report(Path::new(""), 0)
}

/// Extracts an archive to a staging directory that must not exist.
/// Progress reports the current archive-relative path and cumulative output bytes.
/// Return `false` to cancel. The caller owns cleanup of partial output.
pub fn extract(
    format: ArchiveFormat,
    source: &Path,
    staging: &Path,
    password: Option<&str>,
    progress: impl FnMut(&Path, u64) -> bool + Send + 'static,
) -> Result<(), ArchiveError> {
    let password = checked_password(format, password)?;
    let progress = Progress::new(progress);
    progress.report(Path::new(""), 0)?;
    let file = File::open(source)?;
    fs::create_dir(staging)?;
    let mut output = Output::new(staging);
    match format {
        ArchiveFormat::Zip => zip::extract(file, &mut output, password, &progress)?,
        ArchiveFormat::SevenZip => seven_zip::extract(file, &mut output, password, &progress)?,
        ArchiveFormat::Rar => rar::extract(file, &mut output, password, &progress)?,
        ArchiveFormat::Tar => tarball::extract(file, &mut output, false, &progress)?,
        ArchiveFormat::TarGz => tarball::extract(file, &mut output, true, &progress)?,
    }
    progress.report(Path::new(""), 0)
}

fn checked_password(
    format: ArchiveFormat,
    password: Option<&str>,
) -> Result<Option<&str>, ArchiveError> {
    let password = password.filter(|value| !value.is_empty());
    if password.is_some() && !format.supports_password() {
        return Err(ArchiveError::UnsupportedEncryption);
    }
    Ok(password)
}

#[derive(Clone)]
struct Progress(Arc<Mutex<ProgressState>>);

type ProgressCallback = dyn FnMut(&Path, u64) -> bool + Send;

struct ProgressState {
    callback: Box<ProgressCallback>,
    path: PathBuf,
    bytes: u64,
    last_report: Instant,
    cancelled: bool,
}

impl Progress {
    fn new(callback: impl FnMut(&Path, u64) -> bool + Send + 'static) -> Self {
        Self(Arc::new(Mutex::new(ProgressState {
            callback: Box::new(callback),
            path: PathBuf::new(),
            bytes: 0,
            last_report: Instant::now(),
            cancelled: false,
        })))
    }

    fn report(&self, path: &Path, bytes: u64) -> Result<(), ArchiveError> {
        let mut state = self.0.lock().expect("archive progress lock");
        if state.path != path {
            state.path.clear();
            state.path.push(path);
        }
        state.bytes = state.bytes.saturating_add(bytes);
        state.notify()
    }

    fn report_position(&self, bytes: u64) -> Result<(), ArchiveError> {
        let mut state = self.0.lock().expect("archive progress lock");
        state.bytes = state.bytes.max(bytes);
        state.notify()
    }

    fn poll(&self) -> Result<(), ArchiveError> {
        let mut state = self.0.lock().expect("archive progress lock");
        if state.cancelled || state.last_report.elapsed() >= Duration::from_millis(100) {
            state.notify()
        } else {
            Ok(())
        }
    }
}

impl ProgressState {
    fn notify(&mut self) -> Result<(), ArchiveError> {
        self.last_report = Instant::now();
        self.cancelled = self.cancelled || !(self.callback)(&self.path, self.bytes);
        if self.cancelled {
            Err(ArchiveError::Cancelled)
        } else {
            Ok(())
        }
    }
}

struct Input<'a> {
    reader: File,
    name: &'a Path,
    progress: &'a Progress,
    bytes: u64,
}

impl Read for Input<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let count = self.reader.read(buffer)?;
        self.bytes += count as u64;
        self.progress
            .report(self.name, count as u64)
            .map_err(io::Error::other)?;
        Ok(count)
    }
}

fn extract_entry(
    output: &mut Output<'_>,
    name: &Path,
    is_directory: bool,
    size: u64,
    reader: &mut dyn Read,
    progress: &Progress,
) -> Result<(), ArchiveError> {
    progress.report(name, 0)?;
    let mut file = output.entry(name, is_directory)?;
    let mut buffer = [0; 64 * 1024];
    let mut bytes = 0;
    loop {
        let count = reader.read(&mut buffer).map_err(read_error)?;
        if count == 0 {
            break;
        }
        bytes += count as u64;
        let Some(file) = &mut file else {
            return Err(ArchiveError::InvalidArchive);
        };
        if bytes > size {
            return Err(ArchiveError::InvalidArchive);
        }
        file.write_all(&buffer[..count])?;
        progress.report(name, count as u64)?;
    }
    if bytes != size {
        return Err(ArchiveError::InvalidArchive);
    }
    Ok(())
}

fn read_error(error: io::Error) -> ArchiveError {
    if error.raw_os_error().is_some() {
        return error.into();
    }
    error
        .downcast::<ArchiveError>()
        .unwrap_or(ArchiveError::InvalidArchive)
}
