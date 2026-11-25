use std::collections::HashSet;
use std::fs::File;
use std::io;

use sevenz_rust2::encoder_options::AesEncoderOptions;
use sevenz_rust2::{
    Archive, ArchiveEntry, ArchiveReader, ArchiveWriter, EncoderMethod, Error, Password,
};

use super::paths::{Output, Source, is_special_mode, relative_path};
use super::{ArchiveError, Progress, extract_entry, read_error};

pub(super) fn create(
    file: File,
    sources: &[Source],
    password: Option<&str>,
    progress: &Progress,
) -> Result<File, ArchiveError> {
    let mut writer = ArchiveWriter::new(file).map_err(write_error)?;
    if let Some(password) = password {
        writer.set_content_methods(vec![
            AesEncoderOptions::new(Password::from(password)).into(),
            EncoderMethod::LZMA2.into(),
        ]);
    }
    for source in sources {
        let bytes = source.name_bytes()?;
        let name = std::str::from_utf8(&bytes)
            .map_err(|_| ArchiveError::UnsupportedEntry(source.path.clone()))?;
        let directory = source.metadata.is_dir();
        progress.report(&source.name, 0)?;
        let entry = if directory {
            ArchiveEntry::new_directory(name)
        } else {
            ArchiveEntry::new_file(name)
        };
        let reader = (!directory).then(|| source.input(progress)).transpose()?;
        writer
            .push_archive_entry(entry, reader)
            .map_err(write_error)?;
    }
    Ok(writer.finish()?)
}

pub(super) fn extract(
    mut file: File,
    output: &mut Output<'_>,
    password: Option<&str>,
    progress: &Progress,
) -> Result<(), ArchiveError> {
    let password = password.map(Password::from).unwrap_or_default();
    let (archive, mut password_verified) = match Archive::read(&mut file, &Password::default()) {
        Ok(archive) => (archive, false),
        Err(Error::PasswordRequired) if !password.is_empty() => (
            Archive::read(&mut file, &password).map_err(extract_error)?,
            true,
        ),
        Err(error) => return Err(extract_error(error)),
    };
    let encrypted: HashSet<_> = archive
        .files
        .iter()
        .enumerate()
        .filter(|(index, entry)| entry.has_stream() && archive.is_file_encrypted(*index))
        .map(|(_, entry)| entry.name().to_owned())
        .collect();
    let mut archive = ArchiveReader::from_archive(archive, file, password);
    archive
        .for_each_entries(|entry, reader| {
            let result = (|| {
                let name = relative_path(entry.name().as_bytes(), true)?;
                if entry.is_anti_item()
                    || (entry.has_windows_attributes
                        && (entry.windows_attributes & 0x0400 != 0
                            || is_special_mode(u64::from(entry.windows_attributes >> 16))))
                {
                    return Err(ArchiveError::UnsupportedEntry(name));
                }
                extract_entry(
                    output,
                    &name,
                    entry.is_directory(),
                    entry.size,
                    reader,
                    progress,
                )
            })();
            if encrypted.contains(entry.name()) {
                // Without a verified header, the first encrypted stream also checks the password.
                if matches!(result, Err(ArchiveError::InvalidArchive)) && !password_verified {
                    return Err(io::Error::other(ArchiveError::InvalidPassword).into());
                }
                password_verified |= result.is_ok();
            }
            result
                .map(|()| true)
                .map_err(|error| io::Error::other(error).into())
        })
        .map_err(extract_error)
}

fn write_error(error: Error) -> ArchiveError {
    match error {
        Error::Io(error, _) => error.into(),
        error => ArchiveError::Io(io::Error::other(error)),
    }
}

fn extract_error(error: Error) -> ArchiveError {
    match error {
        Error::PasswordRequired => ArchiveError::PasswordRequired,
        Error::MaybeBadPassword(error) => error
            .downcast::<ArchiveError>()
            .unwrap_or(ArchiveError::InvalidPassword),
        Error::Io(error, _) => read_error(error),
        _ => ArchiveError::InvalidArchive,
    }
}
