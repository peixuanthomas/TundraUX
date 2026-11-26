use std::path::Path;
use std::sync::Arc;

use hayro::hayro_interpret::{InterpreterSettings, util::TransformExt};
use hayro::hayro_syntax::{LoadPdfError, Pdf, page::Page};
use hayro::kurbo::Affine;
use hayro::vello_cpu::{PixmapMut, RasterizerSettings, RenderContext, Resources, TargetInit};
use i18n::LocalizedText;

const MAX_RASTER_SIDE: f64 = 1600.0;

pub fn is_pdf_path(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"))
}

#[derive(Clone)]
pub struct PdfPreviewDocument(Arc<Pdf>);

impl std::fmt::Debug for PdfPreviewDocument {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PdfPreviewDocument")
            .field("page_count", &self.page_count())
            .finish()
    }
}

impl PartialEq for PdfPreviewDocument {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for PdfPreviewDocument {}

/// An opaque page raster fitted to the viewport's terminal cell proportions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfPageImage {
    pub viewport: [u16; 2],
    pub cell_size: [u16; 2],
    pub cells: [u16; 2],
    pub width: u32,
    pub height: u32,
    pub rgba: Arc<[u8]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PdfPageSize {
    pub pixels: [u16; 2],
    pub cells: [u16; 2],
}

impl PdfPreviewDocument {
    pub fn open(bytes: Vec<u8>) -> Result<Self, LocalizedText> {
        let pdf = std::panic::catch_unwind(|| Pdf::new(bytes))
            .map_err(|_| i18n::msg!("editor-pdf-invalid"))?
            .map_err(|error| match error {
                LoadPdfError::Decryption(_) => i18n::msg!("editor-pdf-encrypted"),
                LoadPdfError::Invalid => i18n::msg!("editor-pdf-invalid"),
            })?;
        if pdf.pages().is_empty() {
            return Err(i18n::msg!("editor-pdf-no-pages").into());
        }
        Ok(Self(Arc::new(pdf)))
    }

    pub fn page_count(&self) -> usize {
        self.0.pages().len()
    }

    pub fn render_page(
        &self,
        index: usize,
        viewport: [u16; 2],
        cell_size: [u16; 2],
    ) -> Result<PdfPageImage, LocalizedText> {
        self.render_pages([index], viewport, cell_size)
            .next()
            .unwrap()
    }

    pub fn page_size(
        &self,
        index: usize,
        viewport: [u16; 2],
        cell_size: [u16; 2],
    ) -> Option<PdfPageSize> {
        fit_page(self.0.pages().get(index)?, viewport, cell_size).map(|(size, _)| size)
    }

    /// Shares Hayro's internal caches within a short batch without retaining them between calls.
    pub fn render_pages(
        &self,
        pages: impl IntoIterator<Item = usize>,
        viewport: [u16; 2],
        cell_size: [u16; 2],
    ) -> impl Iterator<Item = Result<PdfPageImage, LocalizedText>> {
        let mut cache = hayro::RenderCache::new();
        let mut resources = Resources::default();
        pages.into_iter().map(move |index| {
            let error = || i18n::msg!("editor-pdf-render-error", page = index.saturating_add(1));
            let page = self.0.pages().get(index).ok_or_else(error)?;
            let (size, transform) = fit_page(page, viewport, cell_size).ok_or_else(error)?;
            let rgba = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                render_fitted_page(page, size, transform, &cache, &mut resources)
            }))
            .map_err(|_| {
                cache = hayro::RenderCache::new();
                resources = Resources::default();
                error()
            })?;
            Ok(PdfPageImage {
                viewport,
                cell_size,
                cells: size.cells,
                width: u32::from(size.pixels[0]),
                height: u32::from(size.pixels[1]),
                rgba,
            })
        })
    }
}

fn fit_page(
    page: &Page<'_>,
    viewport: [u16; 2],
    cell_size: [u16; 2],
) -> Option<(PdfPageSize, Affine)> {
    let (page_width, page_height) = page.render_dimensions();
    let [page_width, page_height] = [f64::from(page_width), f64::from(page_height)];
    let [cell_width, cell_height] = cell_size.map(f64::from);
    let [columns, rows] = viewport.map(f64::from);
    if [page_width, page_height]
        .into_iter()
        .any(|side| !side.is_finite() || side <= 0.0)
        || viewport.contains(&0)
        || cell_size.contains(&0)
    {
        return None;
    }

    let scale = (columns * cell_width / page_width).min(rows * cell_height / page_height);
    let columns = (page_width * scale / cell_width).ceil().min(columns) as u16;
    let rows = (page_height * scale / cell_height).ceil().min(rows) as u16;
    let (pixel_width, pixel_height) = (
        f64::from(columns) * cell_width,
        f64::from(rows) * cell_height,
    );
    let raster_scale = (MAX_RASTER_SIDE / pixel_width.max(pixel_height)).min(1.0);
    let width = (pixel_width * raster_scale).round().max(1.0) as u16;
    let height = (pixel_height * raster_scale).round().max(1.0) as u16;
    let scale = (scale * raster_scale)
        .min(f64::from(width) / page_width)
        .min(f64::from(height) / page_height);
    // Center the page on whole cells without stretching it to the cell grid.
    let transform = Affine::translate((
        (f64::from(width) - page_width * scale) / 2.0,
        (f64::from(height) - page_height * scale) / 2.0,
    )) * Affine::scale(scale)
        * page.initial_transform(true).to_kurbo();
    Some((
        PdfPageSize {
            pixels: [width, height],
            cells: [columns, rows],
        },
        transform,
    ))
}

fn render_fitted_page<'a>(
    page: &'a Page<'a>,
    size: PdfPageSize,
    transform: Affine,
    cache: &hayro::RenderCache<'a>,
    resources: &mut Resources,
) -> Arc<[u8]> {
    let [width, height] = size.pixels;
    let mut context = RenderContext::new(width, height);
    hayro::render_into(
        page,
        cache,
        &InterpreterSettings::default(),
        &hayro::RenderSettings::default(),
        &mut context,
        transform,
    );
    context.flush();
    // Write into the cached raster's storage to avoid copying the whole page after rendering.
    let bytes = usize::from(width) * usize::from(height) * 4;
    let mut rgba = bytemuck::allocation::zeroed_arc_slice(bytes);
    let pixmap = PixmapMut::new(width, height, Arc::get_mut(&mut rgba).unwrap()).unwrap();
    context.render_with(
        pixmap,
        resources,
        RasterizerSettings {
            target_init: TargetInit::Clear(hayro::vello_cpu::color::palette::css::WHITE),
            ..Default::default()
        },
    );
    rgba
}
