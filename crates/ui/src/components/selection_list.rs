use ratatui::layout::Rect;

pub use super::command_palette::{
    CommandPalette as SelectionList, CommandPaletteCommand as SelectionListItem,
};

pub fn selection_matches<'a>(query: &str, values: impl IntoIterator<Item = &'a str>) -> bool {
    let query = query.trim().to_lowercase();
    query.is_empty()
        || values
            .into_iter()
            .any(|value| value.to_lowercase().contains(&query))
}

/// Search stays above the list even when the dialog has no matching options.
pub fn selection_search_areas(body: Rect) -> (Rect, Rect) {
    let search = Rect::new(body.x, body.y, body.width, body.height.min(1));
    let offset = body.height.min(2);
    (
        search,
        Rect::new(body.x, body.y + offset, body.width, body.height - offset),
    )
}
