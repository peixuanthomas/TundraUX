use crate::BANNER_ASSET_KEY;
use ratatui::style::Color;
use std::io::{self, Write};
use storage::{CLOCK_DESCRIPTOR, CONFIG_DESCRIPTOR, SCHEMA_VERSION};
use terminal_runtime::ansi_foreground;

pub fn banner_lines() -> Result<Vec<String>, ui::AssetError> {
    let ascii_assets = ui::RuntimeAsciiAssets::load_default()?;
    Ok(ascii_assets.banner_lines(BANNER_ASSET_KEY)?.to_vec())
}

pub fn startup_lines() -> Vec<String> {
    vec![
        "TundraUX3 shell - Phase 0 smoke".to_string(),
        "Supported OS: Windows, macOS, and Linux (basic)".to_string(),
        "Target terminal: crossterm-compatible terminal".to_string(),
        format!(
            "Config format: {} (schema v{})",
            CONFIG_DESCRIPTOR.file_name, SCHEMA_VERSION
        ),
        format!(
            "State data: users, state, recent-files, sessions, {} use versioned JSON",
            CLOCK_DESCRIPTOR.name
        ),
    ]
}

pub fn render_static_banner(output: &mut impl Write) -> io::Result<()> {
    render_static_banner_colored(output, Color::White)
}

/// Renders the static logo using `color` as its terminal foreground color.
pub fn render_static_banner_colored(output: &mut impl Write, color: Color) -> io::Result<()> {
    let ascii_assets = ui::RuntimeAsciiAssets::load_default().map_err(asset_io_error)?;
    render_static_banner_with_assets_colored(output, &ascii_assets, color)
}

pub fn render_static_banner_with_assets(
    output: &mut impl Write,
    ascii_assets: &ui::RuntimeAsciiAssets,
) -> io::Result<()> {
    render_static_banner_with_assets_colored(output, ascii_assets, Color::White)
}

/// Renders the supplied static logo using `color` as its terminal foreground color.
pub fn render_static_banner_with_assets_colored(
    output: &mut impl Write,
    ascii_assets: &ui::RuntimeAsciiAssets,
    color: Color,
) -> io::Result<()> {
    write!(output, "{}", ansi_foreground(color))?;
    for line in ascii_assets
        .banner_lines(BANNER_ASSET_KEY)
        .map_err(asset_io_error)?
    {
        writeln!(output, "{line}")?;
    }
    write!(output, "\x1B[0m")?;

    Ok(())
}

pub fn display_banner(output: &mut impl Write) -> io::Result<()> {
    display_banner_colored(output, Color::White)
}

/// Plays the frost startup logo using `color` as its terminal foreground color.
pub fn display_banner_colored(output: &mut impl Write, color: Color) -> io::Result<()> {
    crate::startup_banner::display_startup_banner_colored(output, color)
}

pub(crate) fn asset_io_error(error: ui::AssetError) -> io::Error {
    io::Error::other(error.to_string())
}
