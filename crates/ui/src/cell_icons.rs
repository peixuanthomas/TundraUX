use std::cell::Cell;

pub use ascii_assets::CellIconMode;

thread_local! {
    // Layout, drawing and input share the UI thread, including independent AppHosts.
    static MODE: Cell<CellIconMode> = const { Cell::new(CellIconMode::Ascii) };
}

pub fn set_cell_icon_mode(mode: CellIconMode) {
    MODE.set(mode);
}

pub fn cell_icon_mode() -> CellIconMode {
    MODE.get()
}

pub fn cell_icon(group: &str, role: &str) -> &'static str {
    cell_icon_mode().icon(group, role)
}
