use std::fs::File;
use std::io::{self, Read, Write};
use std::path::Path;

use flate2::Compression;
use flate2::read::MultiGzDecoder;
use flate2::write::GzEncoder;

use super::paths::{Output, Source, relative_path};
use super::{ArchiveError, Progress, extract_entry, read_error};

pub(super) fn create(
    file: File,
    sources: &[Source],
    gzip: bool,
    progress: &Progress,
) -> Result<File, ArchiveError> {
    if gzip {
        let writer = write_tar(
            GzEncoder::new(file, Compression::default()),
            sources,
            progress,
        )?;
        Ok(writer.finish()?)
    } else {
        write_tar(file, sources, progress)
    }
}

fn write_tar<W: Write>(
    writer: W,
    sources: &[Source],
    progress: &Progress,
) -> Result<W, ArchiveError> {
    let mut archive = tar::Builder::new(writer);
    for source in sources {
        let mut header = tar::Header::new_gnu();
        header.set_metadata(&source.metadata);
        if source.metadata.is_dir() {
            progress.report(&source.name, 0)?;
            archive.append_data(&mut header, &source.name, io::empty())?;
        } else {
            let mut input = source.input(progress)?;
            archive.append_data(&mut header, &source.name, &mut input)?;
            if input.bytes != source.metadata.len() {
                return Err(ArchiveError::Io(io::Error::other(
                    "TAR source size changed while reading",
                )));
            }
        }
    }
    Ok(archive.into_inner()?)
}

pub(super) fn extract(
    file: File,
    output: &mut Output<'_>,
    gzip: bool,
    progress: &Progress,
) -> Result<(), ArchiveError> {
    if gzip {
        read_tar(MultiGzDecoder::new(file), output, progress)
    } else {
        read_tar(file, output, progress)
    }
}

fn read_tar<R: Read>(
    reader: R,
    output: &mut Output<'_>,
    progress: &Progress,
) -> Result<(), ArchiveError> {
    let mut archive = tar::Archive::new(reader);
    for entry in archive.entries().map_err(read_error)? {
        let mut entry = entry.map_err(read_error)?;
        let kind = entry.header().entry_type();
        if kind.is_pax_global_extensions() {
            continue;
        }
        let name = relative_path(&entry.path_bytes(), true)?;
        if !(kind.is_dir() || kind.is_file() || kind.is_gnu_sparse()) {
            return Err(ArchiveError::UnsupportedEntry(name));
        }
        extract_entry(
            output,
            &name,
            kind.is_dir(),
            entry.size(),
            &mut entry,
            progress,
        )?;
    }

    // tar stops at the first end marker. Drain the padding to verify the gzip trailer too.
    let mut reader = archive.into_inner();
    let mut end = [0; 512];
    reader.read_exact(&mut end).map_err(read_error)?;
    if end.iter().any(|&byte| byte != 0) {
        return Err(ArchiveError::InvalidArchive);
    }
    let mut buffer = [0; 64 * 1024];
    loop {
        let count = reader.read(&mut buffer).map_err(read_error)?;
        if count == 0 {
            return Ok(());
        }
        if buffer[..count].iter().any(|&byte| byte != 0) {
            return Err(ArchiveError::InvalidArchive);
        }
        progress.report(Path::new(""), 0)?;
    }
}
