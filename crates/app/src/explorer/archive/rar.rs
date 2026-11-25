use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

use rars::{
    ArchiveFamily, ArchiveReadOptions, ArchiveReader, ArchiveVersion, AttrSource, Builder,
    EntrySource, ErrorKind, ExtractionDecision, ReadCancellation, WriteCancellation,
    WriteOperation, WriteProgress, WriteProgressEvent, WriterResources,
};

use super::paths::{Output, Source, is_special_mode, open_regular, relative_path};
use super::{ArchiveError, Progress};

pub(super) fn create(
    mut file: File,
    sources: &[Source],
    password: Option<&str>,
    progress: &Progress,
) -> Result<File, ArchiveError> {
    let mut archive = Builder::new(ArchiveVersion::Rar50)
        .password(password.map(|password| password.as_bytes().to_vec()))
        .header_encryption(password.is_some());
    for source in sources {
        progress.report(&source.name, 0)?;
        let name = rars::filename::encode_rar50(&source.name_bytes()?).into_owned();
        let result = if source.metadata.is_dir() {
            archive.add_directory(name, None, None)
        } else {
            let path = source.path.clone();
            let input = EntrySource::from_opener(source.metadata.len(), move || {
                open_regular(&path)
                    .map(|file| Box::new(file) as Box<dyn rars::EntryReader>)
                    .map_err(|error| io::Error::other(error).into())
            });
            archive.add_source(name, input, None, None)
        };
        result.map_err(write_error)?;
    }
    let reporter = CompressionProgress {
        progress,
        cancellation: WriteCancellation::new(),
    };
    let resources = WriterResources::default().with_cancellation(reporter.cancellation.clone());
    archive
        .write_to(&mut file, &resources, Some(&reporter))
        .map_err(write_error)?;
    Ok(file)
}

pub(super) fn extract(
    file: File,
    output: &mut Output<'_>,
    password: Option<&str>,
    progress: &Progress,
) -> Result<(), ArchiveError> {
    let cancellation = ReadCancellation::new();
    let options = ArchiveReadOptions::with_optional_password(password.map(str::as_bytes))
        .with_cancellation(&cancellation)
        .with_rar50_buffered_decode_limit(32 * 1024 * 1024);
    let reader = ProgressReader {
        file,
        progress: progress.clone(),
        cancellation: cancellation.clone(),
    };
    let archive = ArchiveReader::read_reader_with_options(reader, options)
        .map_err(|error| extract_error(error, &cancellation))?;
    let mut output_error = None;
    let result = archive.extract_with_control(options, |member| {
        let result = (|| {
            let meta = &member.meta;
            let bytes = if meta.family == ArchiveFamily::Rar50Plus {
                rars::filename::decode_rar50(meta.name_bytes())
            } else {
                meta.name_bytes().into()
            };
            let name = relative_path(&bytes, true)?;
            if meta.is_redirection
                || meta.is_split_before
                || meta.is_split_after
                || (meta.attr_source() == AttrSource::Unix && is_special_mode(meta.file_attr))
                || (meta.attr_source() == AttrSource::Dos && meta.file_attr & 0x0400 != 0)
            {
                return Err(ArchiveError::UnsupportedEntry(name));
            }
            progress.report(&name, 0)?;
            let file = output.entry(&name, meta.is_directory)?;
            Ok(match file {
                Some(file) => ExtractionDecision::Extract(Box::new(ProgressWriter {
                    file,
                    name,
                    progress: progress.clone(),
                    cancellation: cancellation.clone(),
                })),
                None => ExtractionDecision::Skip,
            })
        })();
        match result {
            Ok(decision) => Ok(decision),
            Err(error) => {
                output_error = Some(error);
                Ok(ExtractionDecision::Stop)
            }
        }
    });
    if let Some(error) = output_error {
        return Err(error);
    }
    result
        .map(|_| ())
        .map_err(|error| extract_error(error, &cancellation))
}

struct CompressionProgress<'a> {
    progress: &'a Progress,
    cancellation: WriteCancellation,
}

impl WriteProgress for CompressionProgress<'_> {
    fn report(&self, event: WriteProgressEvent<'_>) {
        let result = match event {
            WriteProgressEvent::EntryStarted {
                operation: WriteOperation::Compression,
                name,
                ..
            } => {
                let decoded = rars::filename::decode_rar50(name);
                let Ok(name) = relative_path(&decoded, true) else {
                    return;
                };
                self.progress.report(&name, 0)
            }
            WriteProgressEvent::Advanced {
                operation: WriteOperation::Compression,
                completed_bytes,
                ..
            } => self.progress.report_position(completed_bytes),
            _ => return,
        };
        if result.is_err() {
            self.cancellation.cancel();
        }
    }

    fn is_cancelled(&self) -> bool {
        if self.progress.poll().is_err() {
            self.cancellation.cancel();
        }
        self.cancellation.is_cancelled()
    }
}

struct ProgressReader {
    file: File,
    progress: Progress,
    cancellation: ReadCancellation,
}

impl Read for ProgressReader {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        check_read_progress(self.progress.poll(), &self.cancellation)?;
        self.file.read(bytes)
    }
}

impl Seek for ProgressReader {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        self.file.seek(position)
    }
}

struct ProgressWriter {
    file: File,
    name: PathBuf,
    progress: Progress,
    cancellation: ReadCancellation,
}

impl Write for ProgressWriter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        let count = self.file.write(bytes)?;
        check_read_progress(
            self.progress.report(&self.name, count as u64),
            &self.cancellation,
        )?;
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file.flush()
    }
}

fn check_read_progress(
    result: Result<(), ArchiveError>,
    cancellation: &ReadCancellation,
) -> io::Result<()> {
    result.map_err(|error| {
        cancellation.cancel();
        io::Error::other(error)
    })
}

fn write_error(error: rars::Error) -> ArchiveError {
    match error.kind() {
        ErrorKind::Cancelled => ArchiveError::Cancelled,
        _ => ArchiveError::Io(io::Error::other(error)),
    }
}

fn extract_error(error: rars::Error, cancellation: &ReadCancellation) -> ArchiveError {
    if cancellation.is_cancelled() {
        return ArchiveError::Cancelled;
    }
    match error.kind() {
        ErrorKind::Cancelled => ArchiveError::Cancelled,
        ErrorKind::BadPassword => ArchiveError::InvalidPassword,
        ErrorKind::PasswordRequired => ArchiveError::PasswordRequired,
        ErrorKind::Io => ArchiveError::Io(io::Error::other(error)),
        _ => ArchiveError::InvalidArchive,
    }
}
