use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use app::explorer::archive::{
    ArchiveError, ArchiveFormat, create, default_extract_name, extract, is_recognized_archive,
};

const FORMATS: [ArchiveFormat; 5] = [
    ArchiveFormat::Zip,
    ArchiveFormat::SevenZip,
    ArchiveFormat::Rar,
    ArchiveFormat::Tar,
    ArchiveFormat::TarGz,
];

#[test]
fn recognizes_supported_and_unsupported_archive_names() {
    for (name, format, stem) in [
        ("Report.ZIP", ArchiveFormat::Zip, "Report"),
        ("report.7Z", ArchiveFormat::SevenZip, "report"),
        ("report.RaR", ArchiveFormat::Rar, "report"),
        ("report.TAR", ArchiveFormat::Tar, "report"),
        ("My.Report.Tar.Gz", ArchiveFormat::TarGz, "My.Report"),
        ("report.TGZ", ArchiveFormat::TarGz, "report"),
    ] {
        let path = Path::new(name);
        assert_eq!(ArchiveFormat::from_path(path), Some(format));
        assert_eq!(default_extract_name(path).unwrap(), stem);
        assert!(is_recognized_archive(path));
    }
    for name in ["file.GZ", "report.tar.xz", "report.BZ2", "report.zipx"] {
        let path = Path::new(name);
        assert!(is_recognized_archive(path));
        assert_eq!(ArchiveFormat::from_path(path), None);
    }
    assert_eq!(default_extract_name(Path::new("...zip")), None);
    assert!(!is_recognized_archive(Path::new("file.zip.txt")));
}

#[test]
fn round_trips_multiple_sources_and_empty_entries_with_progress() {
    let work = tempfile::tempdir().unwrap();
    let directory = work.path().join("selected");
    fs::create_dir(&directory).unwrap();
    fs::create_dir(directory.join("empty")).unwrap();
    fs::write(directory.join("empty.txt"), []).unwrap();
    let content = vec![42; 256 * 1024];
    fs::write(directory.join("content.txt"), &content).unwrap();
    let other = work.path().join("第二份.txt");
    fs::write(&other, b"second file").unwrap();
    let sources = [directory, other];
    let total = (content.len() + b"second file".len()) as u64;
    let meter = || {
        let count = Arc::new(AtomicU64::new(0));
        let observed = count.clone();
        let progress = move |_: &Path, bytes: u64| {
            assert!(bytes >= observed.swap(bytes, Ordering::Relaxed));
            true
        };
        (count, progress)
    };

    for format in FORMATS {
        let archive = work.path().join(format!("archive.{}", format.extension()));
        let stage = work.path().join(format!("extract-{}", format.extension()));
        let (written, progress) = meter();
        create(format, &sources, &archive, None, progress).unwrap();
        assert_eq!(written.load(Ordering::Relaxed), total, "{format:?}");

        let (extracted, progress) = meter();
        extract(format, &archive, &stage, None, progress).unwrap();
        assert_eq!(extracted.load(Ordering::Relaxed), total, "{format:?}");
        assert_eq!(
            fs::read(stage.join("selected/content.txt")).unwrap(),
            content
        );
        assert!(stage.join("selected/empty").is_dir());
        assert!(
            fs::read(stage.join("selected/empty.txt"))
                .unwrap()
                .is_empty()
        );
        assert_eq!(fs::read(stage.join("第二份.txt")).unwrap(), b"second file");
    }
}

#[test]
fn encrypts_and_distinguishes_missing_and_incorrect_passwords() {
    let work = tempfile::tempdir().unwrap();
    let source = work.path().join("text.txt");
    fs::write(&source, b"confidential test document").unwrap();
    let password = "样本 p@ssword";
    for format in FORMATS {
        let archive = work
            .path()
            .join(format!("protected.{}", format.extension()));
        let result = create(
            format,
            std::slice::from_ref(&source),
            &archive,
            Some(password),
            |_, _| true,
        );
        if !format.supports_password() {
            assert!(matches!(result, Err(ArchiveError::UnsupportedEncryption)));
            assert!(!archive.exists());
            continue;
        }
        result.unwrap();
        let missing = work.path().join(format!("missing-{}", format.extension()));
        let wrong = work.path().join(format!("wrong-{}", format.extension()));
        let correct = work.path().join(format!("correct-{}", format.extension()));
        let result = extract(format, &archive, &missing, None, |_, _| true);
        assert!(
            matches!(result, Err(ArchiveError::PasswordRequired)),
            "{format:?}: {result:?}"
        );
        let result = extract(format, &archive, &wrong, Some("wrong"), |_, _| true);
        assert!(
            matches!(result, Err(ArchiveError::InvalidPassword)),
            "{format:?}: {result:?}"
        );
        extract(format, &archive, &correct, Some(password), |_, _| true).unwrap();
        assert_eq!(
            fs::read(correct.join("text.txt")).unwrap(),
            b"confidential test document"
        );
    }
}

#[test]
fn reads_independently_created_archives() {
    let work = tempfile::tempdir().unwrap();
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/explorer_archives");
    // Created with 7-Zip 26.06, RAR 7.23, and bsdtar 3.5.3.
    for (filename, format, password) in [
        ("encrypted.zip", ArchiveFormat::Zip, Some("fixture-pass")),
        (
            "encrypted-content.7z",
            ArchiveFormat::SevenZip,
            Some("fixture-pass"),
        ),
        (
            "encrypted-headers.7z",
            ArchiveFormat::SevenZip,
            Some("fixture-pass"),
        ),
        ("encrypted.rar", ArchiveFormat::Rar, Some("fixture-pass")),
        ("external.tar.gz", ArchiveFormat::TarGz, None),
    ] {
        let archive = fixtures.join(filename);
        if password.is_some() {
            let missing = work.path().join(format!("missing-{filename}"));
            let result = extract(format, &archive, &missing, None, |_, _| true);
            assert!(
                matches!(result, Err(ArchiveError::PasswordRequired)),
                "{filename}: {result:?}"
            );
            let wrong = work.path().join(format!("wrong-{filename}"));
            let result = extract(format, &archive, &wrong, Some("incorrect"), |_, _| true);
            assert!(
                matches!(result, Err(ArchiveError::InvalidPassword)),
                "{filename}: {result:?}"
            );
        }
        let output = work.path().join(filename);
        extract(format, &archive, &output, password, |_, _| true)
            .unwrap_or_else(|error| panic!("{filename}: {error:?}"));
        assert_eq!(
            fs::read(output.join("fixture.txt")).unwrap(),
            b"archive fixture\n"
        );
        assert_eq!(
            fs::read(output.join("folder/子项.txt")).unwrap(),
            b"second entry\n"
        );
    }

    // Python's zipfile permits identical names in the central directory.
    let duplicate = extract(
        ArchiveFormat::Zip,
        &fixtures.join("duplicate.zip"),
        &work.path().join("duplicate"),
        None,
        |_, _| true,
    );
    assert!(
        matches!(duplicate, Err(ArchiveError::InvalidArchive)),
        "{duplicate:?}"
    );
}

#[test]
fn cancellation_interrupts_every_format_during_payload_processing() {
    let work = tempfile::tempdir().unwrap();
    let source = work.path().join("large.txt");
    fs::write(&source, vec![42; 128 * 1024]).unwrap();
    let sources = [source];
    for format in FORMATS {
        let partial = work.path().join(format!("partial.{}", format.extension()));
        let cancelled = create(format, &sources, &partial, None, |_, bytes| bytes == 0);
        assert!(
            matches!(cancelled, Err(ArchiveError::Cancelled)),
            "{format:?}: {cancelled:?}"
        );
        let archive = work.path().join(format!("complete.{}", format.extension()));
        create(format, &sources, &archive, None, |_, _| true).unwrap();
        let output = work.path().join(format!("output-{}", format.extension()));
        let cancelled = extract(format, &archive, &output, None, |_, bytes| bytes == 0);
        assert!(
            matches!(cancelled, Err(ArchiveError::Cancelled)),
            "{format:?}: {cancelled:?}"
        );
    }
}

#[test]
fn staging_never_overwrites_an_existing_path_or_includes_itself() {
    let work = tempfile::tempdir().unwrap();
    let source = work.path().join("source");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("data"), b"archive contents").unwrap();
    let archive = source.join("staged.zip");
    let sources = [source];
    create(ArchiveFormat::Zip, &sources, &archive, None, |_, _| true).unwrap();
    let original = fs::read(&archive).unwrap();
    let repeated = create(ArchiveFormat::Zip, &sources, &archive, None, |_, _| true);
    assert!(
        matches!(repeated, Err(ArchiveError::Io(error)) if error.kind() == io::ErrorKind::AlreadyExists)
    );
    assert_eq!(fs::read(&archive).unwrap(), original);

    let ordinary_directory = work.path().join("existing");
    fs::create_dir(&ordinary_directory).unwrap();
    let repeated = extract(
        ArchiveFormat::Zip,
        &archive,
        &ordinary_directory,
        None,
        |_, _| true,
    );
    assert!(
        matches!(repeated, Err(ArchiveError::Io(error)) if error.kind() == io::ErrorKind::AlreadyExists)
    );
    assert_eq!(fs::read_dir(ordinary_directory).unwrap().count(), 0);

    let output = work.path().join("output");
    extract(ArchiveFormat::Zip, &archive, &output, None, |_, _| true).unwrap();
    assert_eq!(
        fs::read(output.join("source/data")).unwrap(),
        b"archive contents"
    );
    assert!(!output.join("source/staged.zip").exists());
}

#[test]
fn detects_corrupt_archives_and_a_damaged_gzip_trailer() {
    let work = tempfile::tempdir().unwrap();
    let corrupt_archive = work.path().join("bad");
    fs::write(&corrupt_archive, b"invalid archive").unwrap();
    for format in FORMATS {
        let output = work.path().join(format!("bad-{}", format.extension()));
        let result = extract(format, &corrupt_archive, &output, None, |_, _| true);
        assert!(
            matches!(result, Err(ArchiveError::InvalidArchive)),
            "{format:?}: {result:?}"
        );
    }

    let source = work.path().join("data");
    let archive = work.path().join("bad-trailer.tar.gz");
    fs::write(&source, b"data in gzip").unwrap();
    create(ArchiveFormat::TarGz, &[source], &archive, None, |_, _| true).unwrap();
    let mut bytes = fs::read(&archive).unwrap();
    *bytes.last_mut().unwrap() ^= 1;
    fs::write(&archive, bytes).unwrap();
    let result = extract(
        ArchiveFormat::TarGz,
        &archive,
        &work.path().join("trailer"),
        None,
        |_, _| true,
    );
    assert!(matches!(result, Err(ArchiveError::InvalidArchive)));
}

#[test]
fn rejects_paths_that_escape_or_collide_and_never_follows_archive_links() {
    let work = tempfile::tempdir().unwrap();
    for (index, name) in [
        "../outside.txt",
        "a/../../outside.txt",
        "./C:\\outside.txt",
        "\\outside.txt",
        "/outside.txt",
    ]
    .into_iter()
    .enumerate()
    {
        let archive = work.path().join(format!("path-{index}.zip"));
        let mut writer = archive_zip::ZipWriter::new(File::create(&archive).unwrap());
        writer
            .start_file(name, archive_zip::write::SimpleFileOptions::default())
            .unwrap();
        writer
            .write_all(b"must stay inside the staging directory")
            .unwrap();
        writer.finish().unwrap();
        let output = work.path().join(format!("path-{index}"));
        let result = extract(ArchiveFormat::Zip, &archive, &output, None, |_, _| true);
        assert!(
            matches!(result, Err(ArchiveError::UnsafePath(_))),
            "{name}: {result:?}"
        );
        assert!(!work.path().join("outside.txt").exists());
    }

    let archive = work.path().join("duplicate.tar");
    let mut writer = tar::Builder::new(File::create(&archive).unwrap());
    for name in ["duplicate", "./duplicate"] {
        let mut header = tar::Header::new_gnu();
        header.set_mode(0o644);
        header.set_size(3);
        writer.append_data(&mut header, name, &b"one"[..]).unwrap();
    }
    writer.finish().unwrap();
    let output = work.path().join("duplicate");
    let result = extract(ArchiveFormat::Tar, &archive, &output, None, |_, _| true);
    assert!(
        matches!(result, Err(ArchiveError::DuplicatePath(_))),
        "{result:?}"
    );

    let archive = work.path().join("link.tar");
    let mut writer = tar::Builder::new(File::create(&archive).unwrap());
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::Symlink);
    header.set_size(0);
    writer
        .append_link(&mut header, "link", "../outside.txt")
        .unwrap();
    writer.finish().unwrap();
    let output = work.path().join("link");
    let result = extract(ArchiveFormat::Tar, &archive, &output, None, |_, _| true);
    assert!(matches!(result, Err(ArchiveError::UnsupportedEntry(_))));
    assert!(!output.join("link").exists());
}
