use std::io::Cursor;
use std::path::{Path, PathBuf};

use office_oxide::ir::{DocumentIR, Element, Image, InlineContent, List};
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
        visit_document_elements(&mut ir, |element| match element {
            Element::Image(image) => image_count += usize::from(image.data.is_some()),
            Element::Paragraph(paragraph) => prepare_inline_text(&mut paragraph.content),
            Element::Heading(heading) => prepare_inline_text(&mut heading.content),
            _ => {}
        });
        let mut images = OfficeImages {
            previews: Vec::new(),
            remaining_images: image_count.min(MAX_IMAGE_DECODES),
            remaining_bytes: MAX_DOCUMENT_IMAGE_BYTES,
        };
        visit_document_elements(&mut ir, |element| {
            if let Element::Image(image) = element {
                images.prepare(image);
            }
        });

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

fn prepare_inline_text(content: &mut [InlineContent]) {
    use office_oxide::ir::VerticalAlign;

    for item in content {
        let InlineContent::Text(span) = item else {
            continue;
        };
        // A script HTML tag would make the rich importer reject the whole table.
        let aligned = match span.vertical_align.take() {
            Some(VerticalAlign::Superscript) => "⁰¹²³⁴⁵⁶⁷⁸⁹⁺⁻⁼⁽⁾⁻",
            Some(VerticalAlign::Subscript) => "₀₁₂₃₄₅₆₇₈₉₊₋₌₍₎₋",
            _ => continue,
        };
        span.text = span
            .text
            .chars()
            .map(|character| {
                "0123456789+-=()−"
                    .chars()
                    .zip(aligned.chars())
                    .find(|(original, _)| *original == character)
                    .map_or(character, |(_, glyph)| glyph)
            })
            .collect();
    }
}

fn visit_document_elements(document: &mut DocumentIR, mut visit: impl FnMut(&mut Element)) {
    for section in &mut document.sections {
        for header in [
            &mut section.first_page_header,
            &mut section.header,
            &mut section.even_page_header,
        ]
        .into_iter()
        .flatten()
        {
            visit_elements(&mut header.content, &mut visit);
        }
        visit_elements(&mut section.elements, &mut visit);
        if let Some(notes) = &mut section.speaker_notes {
            visit_elements(notes, &mut visit);
        }
        for footer in [
            &mut section.first_page_footer,
            &mut section.footer,
            &mut section.even_page_footer,
        ]
        .into_iter()
        .flatten()
        {
            visit_elements(&mut footer.content, &mut visit);
        }
    }
}

fn visit_elements(elements: &mut [Element], visit: &mut impl FnMut(&mut Element)) {
    for element in elements {
        visit(element);
        match element {
            Element::Table(table) => {
                for cell in table.rows.iter_mut().flat_map(|row| &mut row.cells) {
                    visit_elements(&mut cell.content, visit);
                }
            }
            Element::List(list) => visit_list_elements(list, visit),
            Element::TextBox(text_box) => visit_elements(&mut text_box.content, visit),
            Element::Footnote(note) | Element::Endnote(note) => {
                visit_elements(&mut note.content, visit);
            }
            _ => {}
        }
    }
}

fn visit_list_elements(list: &mut List, visit: &mut impl FnMut(&mut Element)) {
    for item in &mut list.items {
        visit_elements(&mut item.content, visit);
        if let Some(nested) = &mut item.nested {
            visit_list_elements(nested, visit);
        }
    }
}
