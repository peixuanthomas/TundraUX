use std::io::{Cursor, Read, Write};
use std::path::Path;
use std::sync::Arc;

use app::editor::{
    DocumentKind, EditorCommand, EditorMode, EditorState, OfficeFormat, PreviewImage,
    rich_document::ProjectedBlockKind,
};
use flate2::read::GzDecoder;
use image::codecs::{gif::GifEncoder, jpeg::JpegEncoder, png::PngEncoder};
use image::{
    DynamicImage, Frame, ImageEncoder, ImageError, ImageFormat, Rgb, RgbImage, Rgba, RgbaImage,
};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

#[test]
fn office_preview_recognizes_document_workbook_and_presentation_variants() {
    for extension in [
        "doc", "dot", "docx", "docm", "dotx", "dotm", "xls", "xlt", "xla", "xlsx", "xlsm", "xltx",
        "xltm", "xlam", "xlsb", "ppt", "pot", "pps", "pptx", "pptm", "potx", "potm", "ppsx",
        "ppsm",
    ] {
        let name = format!("报告.{}", extension.to_ascii_uppercase());
        assert!(
            OfficeFormat::from_path(Path::new(&name)).is_some(),
            "{name}"
        );
    }
    for name in ["报告", "报告.rtf", "报告.ods", "报告.docx.bak", "报告.png"] {
        assert!(OfficeFormat::from_path(Path::new(name)).is_none(), "{name}");
    }
}

#[test]
fn actual_office_documents_preview_their_content_and_keep_the_original_read_only() {
    // The legacy samples are LibreOffice exports of project-authored content.
    const OLE: &[u8] = b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1";
    let doc = include_bytes!("fixtures/office_preview/inspection.doc");
    let dot = include_bytes!("fixtures/office_preview/inspection.dot");
    let xls = include_bytes!("fixtures/office_preview/inventory.xls");
    let mut show = Vec::new();
    GzDecoder::new(&include_bytes!("fixtures/office_preview/handover.pps.gz")[..])
        .read_to_end(&mut show)
        .unwrap();
    assert_ne!(doc, dot);
    for legacy in [
        doc.as_slice(),
        dot.as_slice(),
        xls.as_slice(),
        show.as_slice(),
    ] {
        assert!(legacy.starts_with(OLE));
    }
    let samples: &[(&str, &[u8], &str, &str)] = &[
        ("巡检.DOC", doc, "巡检记录", "继续观察"),
        ("巡检.DOT", dot, "巡检记录", "继续观察"),
        (
            "巡检.docm",
            include_bytes!("fixtures/office_preview/inspection.docm"),
            "巡检记录",
            "继续观察",
        ),
        (
            "巡检.dotx",
            include_bytes!("fixtures/office_preview/inspection.dotx"),
            "巡检记录",
            "继续观察",
        ),
        ("库存.xls", xls, "铅笔", "42"),
        (
            "库存.xltm",
            include_bytes!("fixtures/office_preview/inventory.xltm"),
            "铅笔",
            "42",
        ),
        (
            "库存.XLSB",
            include_bytes!("fixtures/office_preview/inventory.xlsb"),
            "水彩笔",
            "42",
        ),
        ("交接.pps", &show, "核对记录", "完成交接"),
        (
            "交接.potx",
            include_bytes!("fixtures/office_preview/handover.potx"),
            "核对记录",
            "完成交接",
        ),
        (
            "交接.ppsm",
            include_bytes!("fixtures/office_preview/handover.ppsm"),
            "核对记录",
            "完成交接",
        ),
    ];
    let directory = tempfile::tempdir().unwrap();
    for (name, bytes, first, second) in samples.iter().copied() {
        let path = directory.path().join(name);
        std::fs::write(&path, bytes).unwrap();
        let mut editor = OfficeFormat::from_path(&path)
            .unwrap()
            .open_preview(path.clone(), std::fs::read(&path).unwrap())
            .unwrap_or_else(|error| panic!("{name}: {error}"));
        assert_eq!(editor.document.path.as_deref(), Some(path.as_path()));
        assert_eq!(editor.document.kind, DocumentKind::Markdown);
        assert_eq!(editor.mode, EditorMode::Rich);
        assert!(editor.is_read_only());
        let text = editor.export_text();
        assert!(
            text.contains(first) && text.contains(second),
            "{name}: {text}"
        );
        for command in [
            EditorCommand::InsertText("unexpected edit".into()),
            EditorCommand::RequestSave,
            EditorCommand::RequestSaveAs,
        ] {
            assert!(editor.apply(command).is_empty(), "{name}");
        }
        assert_eq!(editor.export_text(), text);
        assert_eq!(std::fs::read(path).unwrap(), bytes);
    }
}

#[test]
fn office_preview_rejects_invalid_documents_and_reports_an_unreadable_binary_workbook() {
    for name in [
        "bad.doc", "bad.dot", "bad.docm", "bad.xls", "bad.xlsx", "bad.ppt", "bad.ppsm",
    ] {
        let path = Path::new(name);
        let format = OfficeFormat::from_path(path).unwrap();
        assert!(
            format
                .open_preview(path.into(), b"not an Office document".to_vec())
                .is_err()
        );
    }

    let mut archive = ZipWriter::new(Cursor::new(Vec::new()));
    archive
        .start_file("xl/workbook.bin", SimpleFileOptions::default())
        .unwrap();
    archive.write_all(b"damaged BIFF12 records").unwrap();
    let bytes = archive.finish().unwrap().into_inner();
    let path = Path::new("damaged.XLSB");
    let format = OfficeFormat::from_path(path).unwrap();
    assert!(matches!(
        format.open_preview(path.into(), bytes),
        Err(office_oxide::OfficeError::Incomplete(_))
    ));

    let source = include_bytes!("fixtures/office_preview/inventory-picture.xlsx");
    let mut original = ZipArchive::new(Cursor::new(source)).unwrap();
    let mut broken = ZipWriter::new(Cursor::new(Vec::new()));
    for index in 0..original.len() {
        let mut part = original.by_index(index).unwrap();
        let patch = match part.name() {
            "xl/workbook.xml" => Some((
                "</sheets>",
                r#"<sheet name="Missing" sheetId="2" r:id="rIdMissing"/></sheets>"#,
            )),
            "xl/_rels/workbook.xml.rels" => Some((
                "</Relationships>",
                concat!(
                    r#"<Relationship Id="rIdMissing" Target="worksheets/sheet2.xml" "#,
                    r#"Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet"/>"#,
                    "</Relationships>"
                ),
            )),
            _ => None,
        };
        if let Some((end, replacement)) = patch {
            let mut xml = String::new();
            part.read_to_string(&mut xml).unwrap();
            broken
                .start_file(part.name(), SimpleFileOptions::default())
                .unwrap();
            broken
                .write_all(xml.replace(end, replacement).as_bytes())
                .unwrap();
        } else {
            broken.raw_copy_file(part).unwrap();
        }
    }
    let bytes = broken.finish().unwrap().into_inner();
    assert!(
        office_oxide::Document::from_reader(
            Cursor::new(bytes.clone()),
            office_oxide::DocumentFormat::Xlsx
        )
        .is_ok()
    );
    let path = Path::new("missing-sheet.xlsx");
    let format = OfficeFormat::from_path(path).unwrap();
    assert!(matches!(
        format.open_preview(path.into(), bytes),
        Err(office_oxide::OfficeError::Incomplete(reason)) if reason.contains("Missing")
    ));
}

#[test]
fn office_preview_reads_worksheet_drawings_in_xlsx_and_binary_xlsb() {
    let spreadsheets: &[(&str, &[u8], &str)] = &[
        (
            "图表.xlsx",
            include_bytes!("fixtures/office_preview/inventory-picture.xlsx"),
            "第三行",
        ),
        (
            "图表.xlsb",
            include_bytes!("fixtures/office_preview/inventory-picture.xlsb"),
            "水彩笔",
        ),
    ];
    for (name, bytes, cell) in spreadsheets {
        let path = Path::new(name);
        let editor = OfficeFormat::from_path(path)
            .unwrap()
            .open_preview(path.into(), bytes.to_vec())
            .unwrap();
        assert!(editor.export_text().contains(cell));
        let urls = picture_urls(&editor);
        assert_eq!(urls.len(), 1, "{name}");
        let picture = editor.office_image(&urls[0]).unwrap();
        assert_eq!((picture.width, picture.height), (2, 1));
        assert_eq!(picture.sample(0, 0, 1, 1), [12, 48, 144, 255]);
    }
}

#[test]
fn office_preview_keeps_decoded_images_in_document_order_and_preserves_alt_text() {
    let path = Path::new("图文.docx");
    let editor = OfficeFormat::from_path(path)
        .unwrap()
        .open_preview(
            path.into(),
            include_bytes!("fixtures/office_preview/picture-order.docx").to_vec(),
        )
        .unwrap();
    let markdown = editor.export_text();
    let positions: Vec<_> = [
        "第一段",
        "蓝色插图",
        "第二段",
        "损坏插图",
        "链接插图",
        "红色插图",
        "最后一段",
    ]
    .into_iter()
    .map(|text| {
        markdown
            .find(text)
            .unwrap_or_else(|| panic!("{text}: {markdown}"))
    })
    .collect();
    assert!(positions.windows(2).all(|pair| pair[0] < pair[1]));
    assert!(!markdown.contains("https://example.invalid/remote.png"));
    assert!(markdown.contains(i18n::tr!("shell-editor-office-image-unavailable").as_str()));

    let urls = picture_urls(&editor);
    assert_eq!(urls.len(), 2);
    let first = editor.office_image(&urls[0]).unwrap();
    assert_eq!((first.width, first.height), (2, 1));
    assert_eq!(first.sample(0, 0, 1, 1), [12, 48, 144, 255]);
    assert!(first.is_opaque());
    assert!(Arc::ptr_eq(&first.rgba, &first.opaque_rgba([3, 6, 9])));
    let second = editor.office_image(&urls[1]).unwrap();
    assert_eq!(second.sample(1, 0, 2, 1), [210, 0, 0, 128]);
    assert!(!second.is_opaque());
    assert!(
        editor
            .office_image("https://example.invalid/remote.png")
            .is_none()
    );
}

#[test]
fn transparent_preview_caches_the_latest_background_without_changing_source_pixels() {
    let path = Path::new("图文.docx");
    let editor = OfficeFormat::from_path(path)
        .unwrap()
        .open_preview(
            path.into(),
            include_bytes!("fixtures/office_preview/picture-order.docx").to_vec(),
        )
        .unwrap();
    let urls = picture_urls(&editor);
    let image = editor.office_image(&urls[1]).unwrap();
    let copy = image.clone();
    let initial = image.opaque_rgba([20, 200, 40]);
    assert_eq!(&initial[..4], &[115, 100, 20, 255]);
    assert!(Arc::ptr_eq(&initial, &copy.opaque_rgba([20, 200, 40])));
    let black = image.opaque_rgba([0, 0, 0]);
    assert_eq!(&black[..4], &[105, 0, 0, 255]);
    assert!(!Arc::ptr_eq(&initial, &image.opaque_rgba([20, 200, 40])));
    assert_eq!(image.sample(0, 0, 1, 1), [210, 0, 0, 128]);
    assert_eq!(image, &copy);
}

#[test]
fn office_preview_reserves_room_for_all_pictures_in_a_document_with_many_images() {
    let path = Path::new("图片集合.docx");
    let editor = OfficeFormat::from_path(path)
        .unwrap()
        .open_preview(
            path.into(),
            include_bytes!("fixtures/office_preview/many-pictures.docx").to_vec(),
        )
        .unwrap();
    let urls = picture_urls(&editor);
    assert_eq!(urls.len(), 76);
    let images: Vec<_> = urls
        .iter()
        .map(|url| editor.office_image(url).unwrap())
        .collect();
    assert!(
        images[..70]
            .iter()
            .all(|image| (image.width, image.height) == (2, 1))
    );
    for photo in &images[70..] {
        assert_eq!(photo.width, photo.height);
        assert!((256..1536).contains(&photo.width));
        assert_eq!(photo.sample(0, 0, 1, 1), [43, 74, 111, 255]);
    }
    let rgba_bytes: usize = images.iter().map(|image| image.rgba.len()).sum();
    assert!(rgba_bytes <= 32 * 1024 * 1024);
}

#[test]
fn image_preview_decodes_supported_rasters_and_selects_the_first_animation_frame() {
    let pixels = RgbImage::from_pixel(3, 2, Rgb([32, 64, 96]));
    let image = DynamicImage::ImageRgb8(pixels);
    for format in [
        ImageFormat::Png,
        ImageFormat::Jpeg,
        ImageFormat::Gif,
        ImageFormat::WebP,
        ImageFormat::Bmp,
        ImageFormat::Tiff,
        ImageFormat::Ico,
    ] {
        let mut bytes = Cursor::new(Vec::new());
        if format == ImageFormat::Ico {
            DynamicImage::ImageRgba8(image.to_rgba8())
                .write_to(&mut bytes, format)
                .unwrap();
        } else {
            image.write_to(&mut bytes, format).unwrap();
        }
        let decoded = PreviewImage::decode(bytes.get_ref())
            .unwrap_or_else(|error| panic!("{format:?}: {error}"));
        assert_eq!((decoded.width, decoded.height), (3, 2), "{format:?}");
        assert_eq!(decoded.rgba.len(), 24);
        assert!(decoded.is_opaque());
    }

    let mut gif = Vec::new();
    let mut encoder = GifEncoder::new(&mut gif);
    for color in [[255, 0, 0, 255], [0, 0, 255, 255]] {
        encoder
            .encode_frame(Frame::new(RgbaImage::from_pixel(2, 2, Rgba(color))))
            .unwrap();
    }
    drop(encoder);
    let first = PreviewImage::decode(&gif).unwrap();
    assert_eq!(first.sample(0, 0, 1, 1), [255, 0, 0, 255]);

    let apng = include_bytes!("fixtures/office_preview/first-animation-frame.png");
    let first = PreviewImage::decode(apng).unwrap();
    assert_eq!(first.sample(0, 0, 1, 1), [0, 255, 0, 255]);
}

#[test]
fn image_preview_resizes_with_aspect_ratio_and_applies_exif_orientation() {
    let pixels = RgbaImage::from_pixel(3072, 2, Rgba([17, 80, 124, 120]));
    let mut png = Vec::new();
    PngEncoder::new(&mut png)
        .write_image(
            pixels.as_raw(),
            pixels.width(),
            pixels.height(),
            image::ExtendedColorType::Rgba8,
        )
        .unwrap();
    let decoded = PreviewImage::decode(&png).unwrap();
    assert_eq!((decoded.width, decoded.height), (1536, 1));
    assert_eq!(decoded.sample(0, 0, 1, 1), [17, 80, 124, 120]);
    assert!(!decoded.is_opaque());

    let mut jpeg = Vec::new();
    let mut encoder = JpegEncoder::new(&mut jpeg);
    // TIFF Exif Orientation 6 rotates the stored pixels clockwise.
    encoder
        .set_exif_metadata(
            b"II*\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x06\0\0\0\0\0\0\0".to_vec(),
        )
        .unwrap();
    encoder
        .write_image(&[90; 24], 4, 2, image::ExtendedColorType::Rgb8)
        .unwrap();
    let decoded = PreviewImage::decode(&jpeg).unwrap();
    assert_eq!((decoded.width, decoded.height), (2, 4));
}

#[test]
fn image_preview_rejects_invalid_data_and_decoded_dimensions_over_budget() {
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageRgb8(RgbImage::from_pixel(1, 1, Rgb([0, 0, 0])))
        .write_to(&mut bytes, ImageFormat::Bmp)
        .unwrap();
    let mut bmp = bytes.into_inner();
    bmp[18..22].copy_from_slice(&8_000i32.to_le_bytes());
    bmp[22..26].copy_from_slice(&8_000i32.to_le_bytes());
    assert!(matches!(
        PreviewImage::decode(&bmp),
        Err(ImageError::Limits(_))
    ));
    assert!(PreviewImage::decode(b"not an image").is_err());
}

#[test]
fn standalone_image_keeps_its_image_reference_when_rich_view_is_rebuilt() {
    let image = PreviewImage::decode(include_bytes!(
        "fixtures/office_preview/first-animation-frame.png"
    ))
    .unwrap();
    let mut editor = EditorState::from_preview_image("插图.png".into(), image.clone());
    assert!(editor.is_read_only());
    assert_eq!(editor.document.path.as_deref(), Some(Path::new("插图.png")));
    assert!(editor.apply(EditorCommand::RequestSave).is_empty());

    editor.apply(EditorCommand::SetMode(EditorMode::Source));
    assert_eq!(editor.mode, EditorMode::Source);
    editor.apply(EditorCommand::SetMode(EditorMode::Rich));
    assert_eq!(editor.mode, EditorMode::Rich);
    let urls = picture_urls(&editor);
    assert_eq!(urls.len(), 1);
    assert_eq!(editor.office_image(&urls[0]), Some(&image));
}

fn picture_urls(editor: &EditorState) -> Vec<String> {
    editor
        .rich_projection()
        .unwrap()
        .blocks
        .into_iter()
        .filter_map(|block| match block.kind {
            ProjectedBlockKind::Paragraph { content } => Some(content),
            _ => None,
        })
        .flatten()
        .filter_map(|span| span.image.map(|image| image.url))
        .collect()
}
