use std::fs::File;
use std::io;

use archive_zip::result::ZipError;
use archive_zip::write::SimpleFileOptions;
use archive_zip::{AesMode, CompressionMethod, ZipArchive, ZipWriter};

use super::paths::{Output, Source, is_special_mode, relative_path};
use super::{ArchiveError, Progress, extract_entry, read_error};

pub(super) fn create(
    file: File,
    sources: &[Source],
    password: Option<&str>,
    progress: &Progress,
) -> Result<File, ArchiveError> {
    let mut writer = ZipWriter::new(file);
    for source in sources {
        let bytes = source.name_bytes()?;
        let name = std::str::from_utf8(&bytes)
            .map_err(|_| ArchiveError::UnsupportedEntry(source.path.clone()))?;
        if source.metadata.is_dir() {
            progress.report(&source.name, 0)?;
            writer
                .add_directory(name, SimpleFileOptions::default())
                .map_err(write_error)?;
            continue;
        }
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .large_file(source.metadata.len() >= u64::from(u32::MAX));
        let options = if let Some(password) = password {
            options.with_aes_encryption(AesMode::Aes256, password)
        } else {
            options
        };
        writer.start_file(name, options).map_err(write_error)?;
        io::copy(&mut source.input(progress)?, &mut writer)?;
    }
    writer.finish().map_err(write_error)
}

pub(super) fn extract(
    file: File,
    output: &mut Output<'_>,
    password: Option<&str>,
    progress: &Progress,
) -> Result<(), ArchiveError> {
    let mut archive = ZipArchive::new(file).map_err(extract_error)?;
    if archive.has_duplicate_names() {
        return Err(ArchiveError::InvalidArchive);
    }
    for index in 0..archive.len() {
        let mut entry = if let Some(password) = password {
            archive.by_index_decrypt(index, password.as_bytes())
        } else {
            archive.by_index(index)
        }
        .map_err(extract_error)?;
        let name = relative_path(entry.name().map_err(extract_error)?.as_bytes(), true)?;
        if entry.is_symlink()
            || entry
                .unix_mode()
                .is_some_and(|mode| is_special_mode(u64::from(mode)))
        {
            return Err(ArchiveError::UnsupportedEntry(name));
        }
        extract_entry(
            output,
            &name,
            entry.is_dir(),
            entry.size(),
            &mut entry,
            progress,
        )?;
    }
    Ok(())
}

fn write_error(error: ZipError) -> ArchiveError {
    match error {
        ZipError::Io(error) => error.into(),
        error => ArchiveError::Io(io::Error::other(error)),
    }
}

fn extract_error(error: ZipError) -> ArchiveError {
    match error {
        ZipError::UnsupportedArchive(ZipError::PASSWORD_REQUIRED) => ArchiveError::PasswordRequired,
        ZipError::InvalidPassword => ArchiveError::InvalidPassword,
        ZipError::Io(error) => read_error(error),
        _ => ArchiveError::InvalidArchive,
    }
}
