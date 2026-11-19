use std::io::Cursor;
use std::path::{Path, PathBuf};

use office_oxide::{Document, DocumentFormat, OfficeError, options::ParseOptions};

use super::{DocumentKind, EditorDocument, EditorState};

#[derive(Debug, Clone, Copy)]
pub struct OfficeFormat(DocumentFormat);

impl OfficeFormat {
    pub fn from_path(path: &Path) -> Option<Self> {
        let extension = path.extension()?.to_str()?;
        if extension.eq_ignore_ascii_case("docx") {
            Some(Self(DocumentFormat::Docx))
        } else if extension.eq_ignore_ascii_case("xlsx") {
            Some(Self(DocumentFormat::Xlsx))
        } else {
            None
        }
    }

    pub fn open_preview(self, path: PathBuf, bytes: Vec<u8>) -> Result<EditorState, OfficeError> {
        let markdown =
            Document::from_reader_with(Cursor::new(bytes), self.0, &ParseOptions::strict())?
                .to_markdown();
        let document = EditorDocument::from_text(Some(path), DocumentKind::Markdown, markdown);
        Ok(EditorState::from_read_only_document(document))
    }
}
