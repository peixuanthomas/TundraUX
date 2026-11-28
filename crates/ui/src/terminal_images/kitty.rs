use std::borrow::Cow;
use std::fmt::Write as _;
use std::io::Write as _;

use flate2::{Compression, write::ZlibEncoder};
use image::DynamicImage;
use ratatui::{
    buffer::Buffer,
    layout::{Rect, Size},
    widgets::Widget,
};
use ratatui_image::{
    Image,
    picker::cap_parser::Parser,
    protocol::{Protocol, kitty::Kitty},
};

pub(super) fn encode(
    rgba: &[u8],
    pixels: [u32; 2],
    size: Size,
    id: u32,
    tmux: bool,
) -> Option<(Kitty, String)> {
    let mut payload = Cow::Borrowed(rgba);
    let mut compression = "";
    if rgba.len() >= 64 * 1024 {
        let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
        encoder.write_all(rgba).ok()?;
        let compressed = encoder.finish().ok()?;
        if compressed.len() <= rgba.len() / 4 * 3 {
            payload = Cow::Owned(compressed);
            compression = "o=z,";
        }
    }

    let kitty = Kitty::new(DynamicImage::new_rgba8(1, 1), size, id, tmux).ok()?;
    // Keep ratatui-image's placement rendering but discard its one-shot dummy upload.
    let scratch = Rect::new(0, 0, 1, 1);
    Image::new(&Protocol::Kitty(kitty.clone()))
        .allow_clipping(true)
        .render(scratch, &mut Buffer::empty(scratch));

    let [width, height] = pixels;
    let Size {
        width: cols,
        height: rows,
    } = size;
    let (start, escape, end) = Parser::tmux_start_escape_end(tmux);
    let mut upload = String::with_capacity(payload.len().div_ceil(3) * 4);
    let chunks = payload.chunks(3072);
    let last = chunks.len() - 1;
    for (index, chunk) in chunks.enumerate() {
        let _ = write!(upload, "{start}{escape}_Gq=2,");
        if index == 0 {
            let _ = write!(
                upload,
                "i={id},a=T,U=1,f=32,t=d,s={width},v={height},c={cols},r={rows},{compression}"
            );
        }
        let _ = write!(upload, "m={};", u8::from(index != last));
        base64_simd::STANDARD.encode_append(chunk, &mut upload);
        let _ = write!(upload, "{escape}\\{end}");
    }
    Some((kitty, upload))
}
