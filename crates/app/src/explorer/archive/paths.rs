use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, Metadata, OpenOptions};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use super::{ArchiveError, Input, Progress};

pub(super) struct Source {
    pub(super) path: PathBuf,
    pub(super) name: PathBuf,
    pub(super) metadata: Metadata,
}

impl Source {
    pub(super) fn name_bytes(&self) -> Result<Cow<'_, [u8]>, ArchiveError> {
        let bytes = rars::filename::native_bytes(self.name.as_os_str())
            .map_err(|_| ArchiveError::UnsupportedEntry(self.path.clone()))?;
        if cfg!(windows) && bytes.contains(&b'\\') {
            Ok(Cow::Owned(
                bytes
                    .iter()
                    .map(|&byte| if byte == b'\\' { b'/' } else { byte })
                    .collect(),
            ))
        } else {
            Ok(Cow::Borrowed(bytes))
        }
    }

    pub(super) fn input<'a>(&'a self, progress: &'a Progress) -> Result<Input<'a>, ArchiveError> {
        progress.report(&self.name, 0)?;
        Ok(Input {
            reader: open_regular(&self.path)?,
            name: &self.name,
            progress,
            bytes: 0,
        })
    }
}

pub(super) fn collect_sources(
    sources: &[PathBuf],
    progress: &Progress,
) -> Result<Vec<Source>, ArchiveError> {
    let mut pending = Vec::with_capacity(sources.len());
    for path in sources.iter().rev() {
        let name = path
            .file_name()
            .ok_or_else(|| ArchiveError::UnsafePath(path.clone()))?;
        pending.push((path.clone(), PathBuf::from(name)));
    }

    let mut seen = HashSet::new();
    let mut output = Vec::new();
    while let Some((path, name)) = pending.pop() {
        progress.report(&name, 0)?;
        let basename = name.file_name().expect("source has an archive name");
        let bytes = basename.as_encoded_bytes();
        if !valid_component(bytes) || bytes.contains(&b'\\') {
            return Err(ArchiveError::UnsafePath(name));
        }
        if !seen.insert(name.clone()) {
            return Err(ArchiveError::DuplicatePath(name));
        }
        let metadata = fs::symlink_metadata(&path)?;
        if !(metadata.is_dir() || metadata.is_file()) || is_link(&metadata) {
            return Err(ArchiveError::UnsupportedEntry(path));
        }
        if metadata.is_dir() {
            let mut children = fs::read_dir(&path)?.collect::<Result<Vec<_>, _>>()?;
            children.sort_unstable_by_key(|child| child.file_name());
            pending.extend(children.into_iter().rev().map(|child| {
                let child_name = name.join(child.file_name());
                (child.path(), child_name)
            }));
        }
        output.push(Source {
            path,
            name,
            metadata,
        });
    }
    Ok(output)
}

pub(super) fn open_regular(path: &Path) -> Result<File, ArchiveError> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || is_link(&metadata) {
        return Err(ArchiveError::UnsupportedEntry(path.to_path_buf()));
    }
    Ok(File::open(path)?)
}

fn is_link(metadata: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x0400 != 0 {
            return true;
        }
    }
    metadata.file_type().is_symlink()
}

pub(super) fn relative_path(
    name: &[u8],
    backslash_separator: bool,
) -> Result<PathBuf, ArchiveError> {
    let separator = |part: u8| part == b'/' || (backslash_separator && part == b'\\');
    let unsafe_path =
        || ArchiveError::UnsafePath(PathBuf::from(String::from_utf8_lossy(name).as_ref()));
    if name.first().is_none_or(|&first| separator(first)) {
        return Err(unsafe_path());
    }
    let mut path = PathBuf::new();
    for part in name.split(|&byte| separator(byte)) {
        if part.is_empty() || part == b"." {
            continue;
        }
        if !valid_component(part) {
            return Err(unsafe_path());
        }
        let part = rars::filename::native_str(part).map_err(|_| unsafe_path())?;
        path.push(part);
    }
    Ok(path)
}

fn valid_component(part: &[u8]) -> bool {
    if part == b".."
        || part.contains(&0)
        || matches!(part, [drive, b':', ..] if drive.is_ascii_alphabetic())
    {
        return false;
    }
    if !cfg!(windows) {
        return true;
    }
    if part
        .iter()
        .any(|&byte| byte < 32 || b"\\<>:\"|?*".contains(&byte))
        || part.ends_with(b".")
        || part.ends_with(b" ")
    {
        return false;
    }
    let stem = part
        .split(|&byte| byte == b'.')
        .next()
        .unwrap_or_default()
        .trim_ascii_end();
    ![
        b"CON".as_slice(),
        b"PRN",
        b"AUX",
        b"NUL",
        b"CONIN$",
        b"CONOUT$",
    ]
    .iter()
    .any(|device| stem.eq_ignore_ascii_case(device))
        && !(stem.len() > 3
            && (stem[..3].eq_ignore_ascii_case(b"COM") || stem[..3].eq_ignore_ascii_case(b"LPT"))
            && matches!(
                &stem[3..],
                [b'0'..=b'9'] | b"\xc2\xb9" | b"\xc2\xb2" | b"\xc2\xb3"
            ))
}

pub(super) fn is_special_mode(mode: u64) -> bool {
    !matches!(mode & 0o170000, 0 | 0o040000 | 0o100000)
}

enum Destination {
    Parent,
    Directory,
    File,
}

pub(super) struct Output<'a> {
    root: &'a Path,
    entries: HashMap<PathBuf, Destination>,
}

impl<'a> Output<'a> {
    pub(super) fn new(root: &'a Path) -> Self {
        Self {
            root,
            entries: HashMap::new(),
        }
    }

    pub(super) fn entry(
        &mut self,
        name: &Path,
        directory: bool,
    ) -> Result<Option<File>, ArchiveError> {
        if name.as_os_str().is_empty() && !directory {
            return Err(ArchiveError::UnsafePath(name.to_path_buf()));
        }
        let mut parent = PathBuf::new();
        for part in name.parent().into_iter().flat_map(Path::components) {
            parent.push(part);
            self.directory(&parent, false)?;
        }
        if directory {
            self.directory(name, true)?;
            return Ok(None);
        }
        if self.entries.contains_key(name) {
            return Err(ArchiveError::DuplicatePath(name.to_path_buf()));
        }
        let file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(self.root.join(name))
            .map_err(|error| destination_error(error, name))?;
        self.entries.insert(name.to_path_buf(), Destination::File);
        Ok(Some(file))
    }

    fn directory(&mut self, name: &Path, explicit: bool) -> Result<(), ArchiveError> {
        let target = self.root.join(name);
        let entry = self.entries.get_mut(name);
        match entry {
            Some(Destination::File) => return Err(ArchiveError::DuplicatePath(name.to_path_buf())),
            Some(Destination::Directory) if explicit => {
                return Err(ArchiveError::DuplicatePath(name.to_path_buf()));
            }
            Some(entry) => {
                let metadata = fs::symlink_metadata(&target)?;
                if !metadata.is_dir() || is_link(&metadata) {
                    return Err(ArchiveError::UnsafePath(name.to_path_buf()));
                }
                if explicit {
                    *entry = Destination::Directory;
                }
                return Ok(());
            }
            None => {}
        }
        if !name.as_os_str().is_empty() {
            fs::create_dir(&target).map_err(|error| destination_error(error, name))?;
        }
        let kind = if explicit {
            Destination::Directory
        } else {
            Destination::Parent
        };
        self.entries.insert(name.to_path_buf(), kind);
        Ok(())
    }
}

fn destination_error(error: std::io::Error, name: &Path) -> ArchiveError {
    if error.kind() == ErrorKind::AlreadyExists {
        ArchiveError::DuplicatePath(name.to_path_buf())
    } else {
        error.into()
    }
}
