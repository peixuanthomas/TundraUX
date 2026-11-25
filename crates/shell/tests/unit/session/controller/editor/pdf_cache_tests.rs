use std::fmt::Write as _;

use super::*;

fn blank_document(count: usize) -> PdfPreviewDocument {
    let kids = (3..count + 3)
        .map(|id| format!("{id} 0 R"))
        .collect::<Vec<_>>()
        .join(" ");
    let mut pdf = String::from("%PDF-1.4\n");
    let mut offsets = Vec::new();
    let mut object = |body: &str| {
        offsets.push(pdf.len());
        writeln!(pdf, "{} 0 obj\n{body}\nendobj", offsets.len()).unwrap();
    };
    object("<< /Type /Catalog /Pages 2 0 R >>");
    object(&format!(
        "<< /Type /Pages /Count {count} /Kids [ {kids} ] >>"
    ));
    for _ in 0..count {
        object("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 120 120] >>");
    }
    let xref = pdf.len();
    let size = offsets.len() + 1;
    writeln!(pdf, "xref\n0 {size}\n0000000000 65535 f ").unwrap();
    for offset in offsets {
        writeln!(pdf, "{offset:010} 00000 n ").unwrap();
    }
    writeln!(
        pdf,
        "trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF"
    )
    .unwrap();
    PdfPreviewDocument::open(pdf.into_bytes()).unwrap()
}

#[test]
fn pdf_prefetch_publishes_first_page_while_the_next_page_is_still_running() {
    let document = blank_document(2);
    let viewport = PdfViewport {
        cells: [30, 18],
        cell_size: [8, 16],
    };
    let raster = document
        .render_page(0, viewport.cells, viewport.cell_size)
        .unwrap();
    let image = EditorPdfPageImage::prepare(raster).unwrap();
    let mut preview = EditorPdfPreview::new(document);
    preview.viewport = Some(viewport);
    preview.plan_targets();
    let (sender, completed) = mpsc::channel();
    preview.task.batch = Some(RenderBatch {
        pages: [0, 1].into(),
        completed,
    });

    sender.send(Ok((1, image.clone()))).unwrap();
    assert!(preview.poll(Some(viewport), None));
    assert_eq!(preview.view_model(viewport.cell_size).image.unwrap().0, 1);
    assert_eq!(preview.task.batch.as_ref().unwrap().pages, [1]);

    sender.send(Ok((2, image.clone()))).unwrap();
    assert!(!preview.poll(Some(viewport), None));
    assert_eq!(preview.view_model(viewport.cell_size).image.unwrap().0, 1);
    preview.turn(Next);
    let (id, cached) = preview.view_model(viewport.cell_size).image.unwrap();
    assert_eq!(id, 2);
    assert!(Arc::ptr_eq(&cached.raster.rgba, &image.raster.rgba));
}

#[test]
fn pdf_prefetch_stays_close_to_the_current_page_and_respects_both_cache_limits() {
    let document = blank_document(20);
    assert_eq!(document.page_count(), 20);
    for cells in [[20, 12], [204, 120]] {
        let viewport = PdfViewport {
            cells,
            cell_size: [8, 16],
        };
        let raster = document.render_page(0, cells, viewport.cell_size).unwrap();
        let image = EditorPdfPageImage::prepare(raster).unwrap();
        let mut preview = EditorPdfPreview::new(document.clone());
        preview.viewport = Some(viewport);
        preview.plan_targets();
        assert_eq!(preview.targets, [0, 1, 2]);
        for page in 0..document.page_count() {
            assert_eq!(preview.page, page);
            assert_eq!(preview.targets.first(), Some(&page));
            assert!(
                preview
                    .targets
                    .iter()
                    .all(|&other| other.abs_diff(page) <= 2)
            );
            for target in preview.targets.clone() {
                preview.record(target, Ok((target as u64, image.clone())));
            }
            assert!(
                preview
                    .targets
                    .iter()
                    .all(|&target| preview.cached(target).is_some())
            );
            assert!(preview.cache.len() <= MAX_CACHED_PAGES);
            assert!(preview.cache.len() * image.bytes() <= MAX_CACHE_BYTES);
            preview.turn(Next);
        }
        let bound = MAX_CACHED_PAGES.min(MAX_CACHE_BYTES / image.bytes());
        assert_eq!(preview.cache.len(), bound);
        assert!(preview.cached(0).is_none());
    }
}
