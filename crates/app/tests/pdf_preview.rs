use std::path::Path;

use app::editor::{PdfPreviewDocument, is_pdf_path};

const PREVIEW: &[u8] = include_bytes!("fixtures/pdf/preview.pdf");

#[test]
fn pdf_preview_renders_vector_and_scanned_pages_at_their_own_aspect_ratio() {
    assert!(is_pdf_path(Path::new("report.PdF")));
    assert!(!is_pdf_path(Path::new("report.pdf.txt")));
    let document = PdfPreviewDocument::open(PREVIEW.to_vec()).unwrap();
    assert_eq!(document.page_count(), 2);

    let vector = document.render_page(0, [106, 14], [8, 16]).unwrap();
    assert_eq!(
        (vector.cells, vector.width, vector.height),
        ([21, 14], 168, 224)
    );
    assert!(
        vector
            .rgba
            .chunks_exact(4)
            .any(|pixel| pixel == [0, 0, 128, 255])
    );
    assert!(
        vector
            .rgba
            .chunks_exact(4)
            .any(|pixel| pixel == [255, 255, 255, 255])
    );

    let scan = document.render_page(1, [106, 14], [8, 16]).unwrap();
    assert_eq!((scan.cells, scan.width, scan.height), ([42, 14], 336, 224));
    assert!(
        scan.rgba
            .chunks_exact(4)
            .any(|pixel| pixel == [162, 40, 20, 255])
    );
    assert!(
        scan.rgba
            .chunks_exact(4)
            .any(|pixel| pixel == [236, 231, 214, 255])
    );

    let tall = document.render_page(0, [30, 40], [9, 18]).unwrap();
    assert_eq!((tall.cells, tall.width, tall.height), ([30, 20], 270, 360));
}

#[test]
fn pdf_preview_reports_invalid_documents_and_missing_passwords() {
    assert_eq!(
        PdfPreviewDocument::open(b"%PDF-1.4\nbroken".to_vec()).unwrap_err(),
        i18n::msg!("editor-pdf-invalid").into()
    );
    assert_eq!(
        PdfPreviewDocument::open(include_bytes!("fixtures/pdf/password.pdf").to_vec()).unwrap_err(),
        i18n::msg!("editor-pdf-encrypted").into()
    );
}
