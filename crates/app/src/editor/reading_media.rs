//! Offline raster assets prepared by the Markdown reading worker.

use std::borrow::Cow;
use std::path::{Component, Path, PathBuf, Prefix};
use std::sync::{Arc, LazyLock};

use image::{DynamicImage, RgbaImage};
use latex_rust::{Dim, MathFont, SvgOptions};
use mermaid_rs_renderer::{LayoutConfig, Theme, compute_layout, parse_mermaid_strict};
use percent_encoding::percent_decode_str;
use resvg::tiny_skia::{Color, Pixmap, PixmapMut, Transform};
use resvg::usvg::{self, fontdb};

use super::PreviewImage;
use super::preview_image::MAX_ENCODED_BYTES;

const MAX_SVG_BYTES: usize = 2 * 1024 * 1024;
const MAX_HEADING_BYTES: usize = 1024;
const MAX_MATH_BYTES: usize = 4096;
const MAX_DIAGRAM_BYTES: usize = 16 * 1024;
const MAX_DIAGRAM_ELEMENTS: usize = 256;

static MATH_FONT: LazyLock<Option<MathFont>> = LazyLock::new(|| MathFont::stix_two_math().ok());
static SYSTEM_FONTS: LazyLock<Arc<fontdb::Database>> = LazyLock::new(|| {
    let mut fonts = fontdb::Database::new();
    fonts.load_system_fonts();
    let query = fontdb::Query {
        families: &[fontdb::Family::SansSerif],
        ..fontdb::Query::default()
    };
    if fonts.query(&query).is_none() {
        let family = fonts
            .faces()
            .find(|face| !face.monospaced && face.style == fontdb::Style::Normal)
            .or_else(|| fonts.faces().next())
            .and_then(|face| face.families.first())
            .map(|(name, _)| name.clone());
        if let Some(family) = family {
            fonts.set_sans_serif_family(family);
        }
    }
    Arc::new(fonts)
});

#[derive(Debug, Clone, Copy)]
pub enum ReadingMedia<'a> {
    Image { destination: &'a str },
    Math { source: &'a str, display: bool },
    Mermaid { source: &'a str },
    Heading { text: &'a str, level: u8 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ReadingMediaStyle {
    pub foreground: [u8; 3],
    pub background: [u8; 3],
    pub cell_size: [u16; 2],
}

/// Run off the UI thread. An unavailable or invalid asset keeps its text fallback.
pub fn render(
    media: ReadingMedia<'_>,
    document_path: Option<&Path>,
    style: ReadingMediaStyle,
    budget: usize,
) -> Option<PreviewImage> {
    if budget < 8 {
        return None;
    }
    match media {
        ReadingMedia::Image { destination } => load_image(destination, document_path, budget),
        ReadingMedia::Math { source, display } => render_math(source, display, style, budget),
        ReadingMedia::Mermaid { source } => render_diagram(source, style, budget),
        ReadingMedia::Heading { text, level } => render_heading(text, level, style, budget),
    }
}

/// Checks whether a Markdown image can name a local file without touching disk.
pub fn is_local_image_reference(destination: &str) -> bool {
    parse_local_image_reference(destination).is_some()
}

fn parse_local_image_reference(destination: &str) -> Option<Cow<'_, str>> {
    let location = destination.split(['?', '#']).next()?;
    let location = percent_decode_str(location).decode_utf8().ok()?;
    let path = Path::new(location.as_ref());
    let disk_path = match path.components().next()? {
        Component::Prefix(prefix)
            if matches!(prefix.kind(), Prefix::Disk(_)) && path.is_absolute() =>
        {
            &location[2..]
        }
        Component::Prefix(_) => return None,
        _ => location.as_ref(),
    };
    let has_scheme_or_stream = if cfg!(windows) {
        disk_path.contains(':')
    } else {
        has_uri_scheme(disk_path)
    };
    if disk_path.contains('\0')
        || has_scheme_or_stream
        || matches!(location.as_bytes(), [b'/' | b'\\', b'/' | b'\\', ..])
        || (path.has_root() && !path.is_absolute())
    {
        return None;
    }
    Some(location)
}

fn local_image_path(destination: &str, document_path: Option<&Path>) -> Option<PathBuf> {
    let location = parse_local_image_reference(destination)?;
    let path = Path::new(location.as_ref());
    if path.is_absolute() {
        return Some(path.into());
    }
    let directory = document_path?.parent()?;
    if matches!(directory.components().next(), Some(Component::Prefix(prefix))
        if !matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)))
    {
        return None;
    }
    Some(directory.join(path))
}

fn has_uri_scheme(path: &str) -> bool {
    path.split_once(':').is_some_and(|(scheme, _)| {
        scheme
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphabetic)
            && scheme
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"+-.".contains(&byte))
    })
}

fn load_image(
    destination: &str,
    document_path: Option<&Path>,
    budget: usize,
) -> Option<PreviewImage> {
    let path = local_image_path(destination, document_path)?;
    let svg = path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("svg"));
    let limit = if svg {
        MAX_SVG_BYTES
    } else {
        MAX_ENCODED_BYTES
    };
    let file = platform::read_document_bytes_limited(&path, limit as u64).ok()?;
    if svg {
        let source = std::str::from_utf8(&file.bytes).ok()?;
        return render_svg(source, true, 1.0, None, budget);
    }
    PreviewImage::decode_with_budget(&file.bytes, budget).ok()
}

fn render_math(
    source: &str,
    display: bool,
    style: ReadingMediaStyle,
    budget: usize,
) -> Option<PreviewImage> {
    if source.len() > MAX_MATH_BYTES {
        return None;
    }
    let [r, g, b] = style.foreground;
    let cell_height = i64::from(style.cell_size[1].max(1));
    let options = SvgOptions {
        font_size_pt: Dim::ratio(cell_height * if display { 9 } else { 6 }, 8),
        color: latex_rust::Color::rgb(r, g, b),
        display,
    };
    let svg = latex_rust::latex_to_svg(source, MATH_FONT.as_ref()?, &options).ok()?;
    render_svg(&svg, false, 1.0, None, budget)
}

fn render_diagram(source: &str, style: ReadingMediaStyle, budget: usize) -> Option<PreviewImage> {
    if source.len() > MAX_DIAGRAM_BYTES {
        return None;
    }
    let graph = parse_mermaid_strict(source).ok()?.graph;
    if graph.nodes.len() + graph.edges.len() + graph.subgraphs.len() > MAX_DIAGRAM_ELEMENTS {
        return None;
    }
    let [r, g, b] = style.background.map(u32::from);
    let mut theme = if 2126 * r + 7152 * g + 722 * b < 128 * 10000 {
        Theme::dark()
    } else {
        Theme::modern()
    };
    theme.background = css_color(style.background);
    theme.primary_text_color = css_color(style.foreground);
    theme.text_color.clone_from(&theme.primary_text_color);
    let config = LayoutConfig::default();
    let layout = compute_layout(&graph, &theme, &config);
    if matches!(
        layout.diagram,
        mermaid_rs_renderer::layout::DiagramData::Error(_)
    ) {
        return None;
    }
    let svg = mermaid_rs_renderer::render_svg(&layout, &theme, &config);
    let scale = f32::from(style.cell_size[1].max(1)) / 18.0;
    render_svg(&svg, true, scale, Some(style.background), budget)
}

fn render_heading(
    text: &str,
    level: u8,
    style: ReadingMediaStyle,
    budget: usize,
) -> Option<PreviewImage> {
    if text.trim().is_empty() || text.len() > MAX_HEADING_BYTES {
        return None;
    }
    let scale = [2.1, 1.8, 1.55, 1.4, 1.25, 1.1][usize::from(level.clamp(1, 6) - 1)];
    let size = f32::from(style.cell_size[1].max(1)) * scale;
    let color = css_color(style.foreground);
    let text = text
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    let svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"><text id="title" font-family="sans-serif" font-weight="bold" font-size="{size}" fill="{color}">{text}</text></svg>"#
    );
    let tree = svg_tree(&svg, true)?;
    let node = tree.node_by_id("title")?;
    let bounds = node.abs_stroke_bounding_box();
    let size = usvg::Size::from_wh(bounds.width() + 2.0, bounds.height() + 2.0)?;
    rasterize(size, 1.0, None, budget, |transform, pixmap| {
        resvg::render(
            &tree,
            transform.pre_translate(1.0 - bounds.x(), 1.0 - bounds.y()),
            pixmap,
        );
    })
}

fn css_color([r, g, b]: [u8; 3]) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn svg_tree(source: &str, load_fonts: bool) -> Option<usvg::Tree> {
    if source.len() > MAX_SVG_BYTES {
        return None;
    }
    let options = usvg::Options {
        font_family: "sans-serif".to_owned(),
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: Box::new(|_, _, _| None),
            resolve_string: Box::new(|_, _| None),
        },
        fontdb: if load_fonts {
            SYSTEM_FONTS.clone()
        } else {
            Arc::new(fontdb::Database::new())
        },
        ..usvg::Options::default()
    };
    let tree = usvg::Tree::from_str(source, &options).ok()?;
    if has_missing_glyphs(tree.root()) || (source.contains("<text") && !tree.has_text_nodes()) {
        return None;
    }
    Some(tree)
}

fn has_missing_glyphs(group: &usvg::Group) -> bool {
    group.children().iter().any(|node| match node {
        usvg::Node::Group(group) => has_missing_glyphs(group),
        usvg::Node::Text(text) => text
            .layouted()
            .iter()
            .flat_map(|span| &span.positioned_glyphs)
            .any(|glyph| glyph.id.0 == 0),
        _ => false,
    })
}

fn render_svg(
    source: &str,
    load_fonts: bool,
    scale: f32,
    background: Option<[u8; 3]>,
    budget: usize,
) -> Option<PreviewImage> {
    let tree = svg_tree(source, load_fonts)?;
    rasterize(
        tree.size(),
        scale,
        background,
        budget,
        |transform, pixmap| {
            resvg::render(&tree, transform, pixmap);
        },
    )
}

fn rasterize(
    size: usvg::Size,
    scale: f32,
    background: Option<[u8; 3]>,
    budget: usize,
    draw: impl FnOnce(Transform, &mut PixmapMut<'_>),
) -> Option<PreviewImage> {
    let maybe_transparent = background.is_none();
    let side = PreviewImage::budgeted_side(budget, maybe_transparent) as f32;
    if side < 1.0 {
        return None;
    }
    let scale = scale.min(side / size.width().max(size.height()));
    let width = (size.width() * scale).ceil().clamp(1.0, side) as u32;
    let height = (size.height() * scale).ceil().clamp(1.0, side) as u32;
    let mut pixmap = Pixmap::new(width, height)?;
    if let Some([r, g, b]) = background {
        pixmap.fill(Color::from_rgba8(r, g, b, 255));
    }
    draw(Transform::from_scale(scale, scale), &mut pixmap.as_mut());
    let mut rgba = pixmap.take();
    if maybe_transparent {
        if rgba.chunks_exact(4).all(|pixel| pixel[3] == 0) {
            return None;
        }
        // The terminal compositor expects straight rather than premultiplied alpha.
        for pixel in rgba.chunks_exact_mut(4) {
            let alpha = u16::from(pixel[3]);
            if (1..255).contains(&alpha) {
                for channel in &mut pixel[..3] {
                    *channel = ((u16::from(*channel) * 255 + alpha / 2) / alpha) as u8;
                }
            }
        }
    }
    let image = DynamicImage::ImageRgba8(RgbaImage::from_raw(width, height, rgba)?);
    Some(PreviewImage::from_pixels(image, maybe_transparent, false))
}
