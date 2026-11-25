use std::io::Cursor;
use std::path::{Path, PathBuf};

use office_oxide::ir::{DocumentIR, Element, Image, List};
use office_oxide::{Document, DocumentFormat, OfficeError, options::ParseOptions};

use super::preview_image::preview_image_url;
use super::{DocumentKind, EditorDocument, EditorState, PreviewImage};

const MAX_IMAGE_DECODES: usize = 2048;
const MAX_DOCUMENT_IMAGE_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Clone, Copy)]
pub struct OfficeFormat(DocumentFormat);

impl OfficeFormat {
    pub fn from_path(path: &Path) -> Option<Self> {
        DocumentFormat::from_path(path).map(Self)
    }

    pub fn open_preview(self, path: PathBuf, bytes: Vec<u8>) -> Result<EditorState, OfficeError> {
        let mut options = ParseOptions::strict();
        options.lazy_pictures = true;
        let mut ir = Document::read_ir_with(Cursor::new(bytes), self.0, &options)?;
        if ir.sections.is_empty()
            && path
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("xlsb"))
        {
            return Err(OfficeError::Incomplete(
                i18n::tr!("shell-editor-office-preview-missing-sheet").into(),
            ));
        }
        let mut image_count = 0;
        visit_document_images(&mut ir, |image| {
            image_count += usize::from(image.data.is_some());
        });
        let mut images = OfficeImages {
            previews: Vec::new(),
            remaining_images: image_count.min(MAX_IMAGE_DECODES),
            remaining_bytes: MAX_DOCUMENT_IMAGE_BYTES,
        };
        visit_document_images(&mut ir, |image| images.prepare(image));

        let document =
            EditorDocument::from_text(Some(path), DocumentKind::Markdown, ir.to_markdown());
        let mut state = EditorState::from_read_only_document(document);
        state.preview_images = images.previews;
        Ok(state)
    }
}

struct OfficeImages {
    previews: Vec<PreviewImage>,
    remaining_images: usize,
    remaining_bytes: usize,
}

impl OfficeImages {
    fn prepare(&mut self, image: &mut Image) {
        image.source_url = image
            .data
            .take()
            .and_then(|bytes| self.store_preview(&bytes));
        if image.source_url.is_none()
            && image
                .alt_text
                .as_deref()
                .is_none_or(|text| text.trim().is_empty())
        {
            image.alt_text = Some(i18n::tr!("shell-editor-office-image-unavailable").into());
        }
    }

    fn store_preview(&mut self, bytes: &[u8]) -> Option<String> {
        let budget = self.remaining_bytes.checked_div(self.remaining_images)?;
        self.remaining_images -= 1;
        let preview = PreviewImage::decode_with_budget(bytes, budget).ok()?;
        self.remaining_bytes -= preview.byte_len();
        let url = preview_image_url(self.previews.len());
        self.previews.push(preview);
        Some(url)
    }
}

fn visit_document_images(document: &mut DocumentIR, mut visit: impl FnMut(&mut Image)) {
    for section in &mut document.sections {
        for header in [
            &mut section.first_page_header,
            &mut section.header,
            &mut section.even_page_header,
        ]
        .into_iter()
        .flatten()
        {
            visit_element_images(&mut header.content, &mut visit);
        }
        visit_element_images(&mut section.elements, &mut visit);
        if let Some(notes) = &mut section.speaker_notes {
            visit_element_images(notes, &mut visit);
        }
        for footer in [
            &mut section.first_page_footer,
            &mut section.footer,
            &mut section.even_page_footer,
        ]
        .into_iter()
        .flatten()
        {
            visit_element_images(&mut footer.content, &mut visit);
        }
    }
}

fn visit_element_images(elements: &mut [Element], visit: &mut impl FnMut(&mut Image)) {
    for element in elements {
        match element {
            Element::Image(image) => visit(image),
            Element::Table(table) => {
                for cell in table.rows.iter_mut().flat_map(|row| &mut row.cells) {
                    visit_element_images(&mut cell.content, visit);
                }
            }
            Element::List(list) => visit_list_images(list, visit),
            Element::TextBox(text_box) => visit_element_images(&mut text_box.content, visit),
            Element::Footnote(note) | Element::Endnote(note) => {
                visit_element_images(&mut note.content, visit);
            }
            _ => {}
        }
    }
}

fn visit_list_images(list: &mut List, visit: &mut impl FnMut(&mut Image)) {
    for item in &mut list.items {
        visit_element_images(&mut item.content, visit);
        if let Some(nested) = &mut item.nested {
            visit_list_images(nested, visit);
        }
    }
}
