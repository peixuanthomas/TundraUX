use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use app::editor::reading_media::{
    ReadingMedia, ReadingMediaStyle, is_local_image_reference, render,
};
use image::{Rgba, RgbaImage};
use latex_rust::ttf_parser::Face;
use resvg::usvg::fontdb::Database;

const BUDGET: usize = 1024 * 1024;
const DARK: ReadingMediaStyle = ReadingMediaStyle {
    foreground: [230, 241, 244],
    background: [7, 17, 22],
    cell_size: [9, 18],
};
const LIGHT: ReadingMediaStyle = ReadingMediaStyle {
    foreground: [24, 26, 32],
    background: [247, 249, 251],
    ..DARK
};

#[test]
fn loads_local_pictures_relative_to_the_document() {
    let (_files, folder) = test_folder();
    let assets = folder.join("assets");
    fs::create_dir(&assets).unwrap();
    let path = assets.join("图 preview.png");
    RgbaImage::from_pixel(40, 30, Rgba([234, 65, 21, 128]))
        .save(&path)
        .unwrap();
    let document = folder.join("readme.md");
    let asset = ReadingMedia::Image {
        destination: "assets/%E5%9B%BE%20preview.png#fragment",
    };
    let image = render(asset, Some(&document), DARK, BUDGET).unwrap();
    assert_eq!([image.width, image.height], [40, 30]);
    assert_eq!(&image.rgba[..4], &[234, 65, 21, 128]);
    assert!(render(asset, None, DARK, BUDGET).is_none());
    assert!(render(asset, Some(&assets.join("not-here.md")), DARK, BUDGET).is_none());
    let absolute = ReadingMedia::Image {
        destination: path.to_str().unwrap(),
    };
    assert_eq!(render(absolute, None, DARK, BUDGET).unwrap(), image);
    let small = render(asset, Some(&document), DARK, 512).unwrap();
    assert!(small.width < image.width);
    assert!(small.byte_len() <= 512);
}

#[test]
fn identifies_local_references_without_requiring_the_files() {
    assert!(is_local_image_reference(
        "../assets/%E5%9B%BE%20preview.png#fragment"
    ));
    assert!(is_local_image_reference("./assets/logo%23dark.svg"));
    for remote in [
        "https://example.invalid/image.png",
        "https%3A%2F%2Fexample.invalid/image.png",
        "data:image/png;base64,AAAA",
        "//example.invalid/image.png",
        "%5C%5Cexample.invalid%5Cimage.png",
        "%00image.png",
        "#fragment",
    ] {
        assert!(!is_local_image_reference(remote), "{remote}");
    }
    if cfg!(windows) {
        assert!(is_local_image_reference(r"C:\assets\logo.svg"));
        assert!(!is_local_image_reference(r"C:\assets\logo.svg:stream"));
        assert!(!is_local_image_reference(r"C:assets\logo.svg"));
    } else {
        assert!(is_local_image_reference("/assets/logo.svg"));
        assert!(is_local_image_reference("./assets/logo:dark.svg"));
    }
}

#[test]
fn remote_and_unreadable_images_have_no_raster_asset() {
    let (files, folder) = test_folder();
    let document = folder.join("readme.md");
    fs::write(folder.join("broken.png"), b"not a picture").unwrap();
    for destination in [
        "https://example.invalid/image.png",
        "//example.invalid/image.png",
        r"\\example.invalid\image.png",
        "%2f%2fexample.invalid/image.png",
        "data:image/png;base64,AAAA",
        "missing.png",
        "broken.png",
    ] {
        let asset = ReadingMedia::Image { destination };
        assert!(render(asset, Some(&document), DARK, BUDGET).is_none());
    }
    drop(files);
}

#[test]
fn local_svg_draws_with_straight_alpha_and_ignores_linked_files() {
    let (_files, folder) = test_folder();
    let document = folder.join("readme.md");
    let source = r#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="30"><rect width="40" height="30" fill="red" opacity="0.5"/></svg>"#;
    fs::write(folder.join("original.svg"), source).unwrap();
    let asset = ReadingMedia::Image {
        destination: "original.svg",
    };
    let image = render(asset, Some(&document), DARK, BUDGET).unwrap();
    assert_eq!([image.width, image.height], [40, 30]);
    assert_eq!(&image.rgba[..4], &[255, 0, 0, 128]);

    let linked = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="30"><image width="40" height="30" href="{}"/></svg>"#,
        folder.join("original.svg").display()
    );
    fs::write(folder.join("linked.svg"), linked).unwrap();
    let asset = ReadingMedia::Image {
        destination: "linked.svg",
    };
    assert!(render(asset, Some(&document), DARK, BUDGET).is_none());
}

#[test]
fn latex_uses_the_theme_and_terminal_cell_height() {
    let source = r"\sum_{k=1}^n k^2";
    let inline = ReadingMedia::Math {
        source,
        display: false,
    };
    let display = ReadingMedia::Math {
        source,
        display: true,
    };
    let normal = render(inline, None, DARK, BUDGET).unwrap();
    let block = render(display, None, DARK, BUDGET).unwrap();
    assert!(block.height > normal.height);
    assert!(has_color(&normal.rgba, DARK.foreground));
    let light = render(inline, None, LIGHT, BUDGET).unwrap();
    assert!(has_color(&light.rgba, LIGHT.foreground));
    let large = render(
        inline,
        None,
        ReadingMediaStyle {
            cell_size: [18, 36],
            ..DARK
        },
        BUDGET,
    )
    .unwrap();
    assert!(large.height >= 2 * normal.height - 1);
    assert!(render(display, None, DARK, 64).is_none_or(|image| image.byte_len() <= 64));
}

#[test]
fn mermaid_uses_the_theme_and_raster_budget() {
    let asset = ReadingMedia::Mermaid {
        source: "flowchart LR\nA[Start] --> B[Done]",
    };
    if !system_fonts_cover("StartDone") {
        assert!(render(asset, None, DARK, BUDGET).is_none());
        return;
    }
    for style in [DARK, LIGHT] {
        let image = render(asset, None, style, BUDGET).unwrap();
        assert!(image.width > image.height);
        assert!(image.is_opaque());
        assert_eq!(&image.rgba[..3], &style.background);
        assert!(has_color(&image.rgba, style.foreground));
        let reduced = render(asset, None, style, 4096).unwrap();
        assert!(reduced.width < image.width);
        assert!(reduced.byte_len() <= 4096);
    }
}

#[test]
fn heading_sizes_and_cjk_follow_available_fonts() {
    let label = "Reading & <Title>";
    let heading = |level| {
        render(
            ReadingMedia::Heading { text: label, level },
            None,
            DARK,
            BUDGET,
        )
    };
    if system_fonts_cover(label) {
        let large = heading(1).unwrap();
        let small = heading(6).unwrap();
        assert!(large.width > small.width);
        assert!(large.height > small.height);
        assert!(has_color(&large.rgba, DARK.foreground));
    } else {
        assert!(heading(1).is_none());
    }

    let cjk = "中文标题";
    for asset in [
        ReadingMedia::Heading {
            text: cjk,
            level: 2,
        },
        ReadingMedia::Mermaid {
            source: "flowchart LR\nA[中文] --> B[标题]",
        },
    ] {
        assert_eq!(
            render(asset, None, DARK, BUDGET).is_some(),
            system_fonts_cover(cjk)
        );
    }
}

#[test]
fn unsupported_or_oversized_source_uses_text_fallback() {
    let large_diagram = format!("flowchart LR\n{}", "A --> B\n".repeat(260));
    let long_formula = "x".repeat(4097);
    for asset in [
        ReadingMedia::Math {
            source: r"\notalatexcommand{1}",
            display: false,
        },
        ReadingMedia::Math {
            source: &long_formula,
            display: true,
        },
        ReadingMedia::Mermaid {
            source: "not a diagram",
        },
        ReadingMedia::Mermaid {
            source: &large_diagram,
        },
        ReadingMedia::Heading {
            text: "  ",
            level: 6,
        },
    ] {
        assert!(render(asset, Some(Path::new("file.md")), DARK, BUDGET).is_none());
    }
}

fn has_color(rgba: &[u8], color: [u8; 3]) -> bool {
    rgba.chunks_exact(4)
        .any(|pixel| pixel[..3] == color && pixel[3] == 255)
}

fn test_folder() -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let path = if cfg!(windows) {
        temp.path().to_path_buf()
    } else {
        temp.path().canonicalize().unwrap()
    };
    (temp, path)
}

fn system_fonts_cover(text: &str) -> bool {
    static FONTS: LazyLock<Database> = LazyLock::new(|| {
        let mut fonts = Database::new();
        fonts.load_system_fonts();
        fonts
    });
    text.chars().all(|character| {
        FONTS.faces().any(|face| {
            FONTS
                .with_face_data(face.id, |data, index| {
                    Face::parse(data, index)
                        .ok()
                        .and_then(|face| face.glyph_index(character))
                        .is_some()
                })
                .unwrap_or(false)
        })
    })
}
