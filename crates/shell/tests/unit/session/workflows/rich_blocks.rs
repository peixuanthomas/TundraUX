use super::*;

use app::editor::markdown_codec::MarkdownCodec;
use app::rich_document::RichLineEnding;

fn projection(source: &str) -> RichProjection {
    MarkdownCodec::import_for_reading(source, false, RichLineEnding::Lf)
        .unwrap()
        .document
        .project()
}

#[test]
fn text_terminal_keeps_equation_source_and_labeled_code_indentation() {
    let projection = projection(concat!(
        "### Title\n\n",
        "Solve $x^2$.\n\n",
        "$$y^2$$\n\n",
        "~~~rust metadata\n\tlet x = \"$literal$\";\n    注釈\n~~~\n\n",
        "~~~math\n\\frac{1}{2}\n~~~\n\n",
        "~~~mermaid\nflowchart TD\n  A --> B\n~~~\n",
    ));
    let blocks = editor_reading_render_blocks(None, &[], &projection, None, false, || false);
    let [heading, inline, display, code, fenced_math, diagram] = blocks.as_slice() else {
        panic!("expected six reading blocks");
    };
    assert!(
        matches!(heading, EditorRenderBlock::Heading { level: 3, spans }
        if spans[0].text == "Title")
    );
    assert!(matches!(inline, EditorRenderBlock::Paragraph(spans)
        if spans.iter().any(|span| span.text == "x^2" && span.italic && !span.inline_code && span.media.is_none())));
    assert!(matches!(display, EditorRenderBlock::Paragraph(spans)
        if spans[0].text == "y^2" && spans[0].italic && !spans[0].inline_code));

    let EditorRenderBlock::RichCodeBlock { language, lines } = code else {
        panic!("expected code block");
    };
    assert_eq!(language.as_deref(), Some("rust"));
    assert_eq!(lines[0].text, "\tlet x = \"$literal$\";");
    assert_eq!(lines[1].text, "    注釈");
    assert_eq!(
        lines[0].rich_range.unwrap().end.grapheme_offset + 1,
        lines[1].rich_range.unwrap().start.grapheme_offset
    );
    assert!(
        matches!(fenced_math, EditorRenderBlock::RichCodeBlock { language: Some(language), lines }
        if language == "math" && lines[0].text == "\\frac{1}{2}")
    );
    assert!(
        matches!(diagram, EditorRenderBlock::RichCodeBlock { language: Some(language), lines }
        if language == "mermaid" && lines[1].text == "  A --> B")
    );
}

#[test]
fn graphics_terminal_skips_native_sized_heading_rasters_and_keeps_invalid_diagrams() {
    let projection = projection(concat!(
        "# Heading\n\n",
        "Inline $x^2$.\n\n",
        "$$y^2$$\n\n",
        "~~~math\n\\frac{1}{2}\n~~~\n\n",
        "~~~mermaid\nflowchart TD\n  A --> B\n~~~\n\n",
        "~~~mermaid\nnot a diagram\n~~~\n",
    ));
    let style = ReadingMediaStyle {
        foreground: [240; 3],
        background: [20; 3],
        cell_size: [8, 16],
    };
    let blocks = editor_reading_render_blocks(None, &[], &projection, Some(style), true, || false);
    let [heading, inline, display, fenced_math, diagram, invalid] = blocks.as_slice() else {
        panic!("expected six reading blocks");
    };
    assert!(matches!(heading, EditorRenderBlock::Heading { .. }));
    assert!(matches!(inline, EditorRenderBlock::Paragraph(spans)
        if spans.iter().any(|span| span.text == "x^2" && span.media.is_some())));
    for block in [display, fenced_math] {
        assert!(matches!(
            block,
            EditorRenderBlock::PreviewGraphic {
                kind: EditorPreviewGraphicKind::Math,
                range: Some(_),
                ..
            }
        ));
    }
    assert!(matches!(
        diagram,
        EditorRenderBlock::PreviewGraphic {
            kind: EditorPreviewGraphicKind::Mermaid,
            ..
        }
    ));
    assert!(
        matches!(invalid, EditorRenderBlock::RichCodeBlock { language: Some(language), lines }
        if language == "mermaid" && lines[0].text == "not a diagram")
    );
}

#[test]
fn markdown_media_only_promotes_local_images_and_keeps_unavailable_labels() {
    let image_bytes = include_bytes!("../../../fixtures/editor/red-blue.png");
    let temp = tempfile::tempdir().unwrap();
    let document_path = temp.path().join("README.md");
    std::fs::write(temp.path().join("red blue.png"), image_bytes).unwrap();
    let projection = projection(concat!(
        "![Local picture](red%20blue.png)\n\n",
        "![Missing picture](missing.png)\n\n",
        "![](https://example.test/picture.png)\n",
    ));
    let blocks =
        editor_reading_render_blocks(Some(&document_path), &[], &projection, None, false, || {
            false
        });
    let [local, missing, remote] = blocks.as_slice() else {
        panic!("expected three image blocks");
    };
    assert!(matches!(local,
        EditorRenderBlock::PreviewGraphic { kind: EditorPreviewGraphicKind::Picture, fallback, range: Some(_), .. }
            if fallback == "Local picture"));
    assert!(matches!(missing, EditorRenderBlock::Paragraph(spans)
        if spans[0].text == "Missing picture"));
    assert!(matches!(remote, EditorRenderBlock::Paragraph(spans)
        if spans[0].text == "https://example.test/picture.png"));
}

#[test]
fn reader_reuses_the_prepared_office_picture() {
    let embedded =
        PreviewImage::decode(include_bytes!("../../../fixtures/editor/red-blue.png")).unwrap();
    let projection = MarkdownCodec::import("![An embedded picture](tundra-preview-image/0)")
        .unwrap()
        .document
        .project();
    let blocks = editor_reading_render_blocks(
        None,
        std::slice::from_ref(&embedded),
        &projection,
        None,
        false,
        || false,
    );
    assert!(
        matches!(&blocks[..], [EditorRenderBlock::Picture { image, .. }]
        if std::sync::Arc::ptr_eq(&image.rgba, &embedded.rgba))
    );
    let (blocks, wants_assets) = editor_reading_text_blocks(
        None,
        std::slice::from_ref(&embedded),
        &projection,
        None,
        false,
        || false,
    );
    assert!(!wants_assets);
    assert!(
        matches!(&blocks[..], [EditorRenderBlock::Picture { image, .. }]
        if Arc::ptr_eq(&image.rgba, &embedded.rgba))
    );
}

#[test]
fn repeated_pictures_and_equations_share_assets_at_distinct_positions() {
    use ratatui::{Terminal, backend::TestBackend, layout::Rect};

    let temp = tempfile::tempdir().unwrap();
    let document_path = temp.path().join("README.md");
    std::fs::write(
        temp.path().join("picture.png"),
        include_bytes!("../../../fixtures/editor/red-blue.png"),
    )
    .unwrap();
    let projection = projection(concat!(
        "![First](picture.png)\n\n",
        "![Second](picture.png)\n\n",
        "Repeat $x^2$ and $x^2$.\n",
    ));
    let style = ReadingMediaStyle {
        foreground: [240; 3],
        background: [20; 3],
        cell_size: [8, 16],
    };
    let blocks = editor_reading_render_blocks(
        Some(&document_path),
        &[],
        &projection,
        Some(style),
        false,
        || false,
    );
    let [
        EditorRenderBlock::PreviewGraphic {
            graphic: first,
            fallback: first_label,
            range: first_range,
            ..
        },
        EditorRenderBlock::PreviewGraphic {
            graphic: second,
            fallback: second_label,
            range: second_range,
            ..
        },
        EditorRenderBlock::Paragraph(spans),
    ] = blocks.as_slice()
    else {
        panic!("expected two picture placements and an inline equation paragraph");
    };
    assert_eq!(
        (first_label.as_str(), second_label.as_str()),
        ("First", "Second")
    );
    assert_ne!(first_range, second_range);
    assert_eq!(first.id, second.id);
    assert!(Arc::ptr_eq(&first.image, &second.image));

    let math = spans
        .iter()
        .filter(|span| span.media.is_some())
        .collect::<Vec<_>>();
    let [first, second] = math.as_slice() else {
        panic!("expected two inline equations");
    };
    assert_ne!(first.rich_range, second.rich_range);
    let first = first.media.as_ref().unwrap();
    let second = second.media.as_ref().unwrap();
    assert_eq!(first.id, second.id);
    assert!(Arc::ptr_eq(&first.image, &second.image));

    let mut model = ui::EditorViewModel::new("README.md", blocks);
    model.read_only = true;
    model.markdown_reading = Some(true);
    model.image_protocol = ui::EditorImageProtocolStatus::Available;
    let area = Rect::new(0, 0, 108, 42);
    let mut terminal = Terminal::new(TestBackend::new(area.width, area.height)).unwrap();
    let mut images = ui::TerminalImageCache::default();
    images.capture_workspace(area);
    let context = ui::RenderContext::from_theme(
        &ui::TundraTheme::default_dark(),
        Default::default(),
        Default::default(),
    );
    terminal
        .draw(|frame| {
            ui::render_editor_contextual_with_terminal_images(
                frame,
                area,
                &model,
                &context,
                &mut images,
                None,
            );
        })
        .unwrap();
    let images = images.take_workspace_images();
    let [first_picture, second_picture, first_math, second_math] = images.as_slice() else {
        panic!("expected four native placements");
    };
    for (first, second) in [(first_picture, second_picture), (first_math, second_math)] {
        assert_eq!(first.image_id, second.image_id);
        assert!(Arc::ptr_eq(&first.rgba, &second.rgba));
        assert_ne!(first.area, second.area);
    }
}

#[test]
fn failed_asset_lookup_is_not_retried_within_one_preparation() {
    let temp = tempfile::tempdir().unwrap();
    let document_path = temp.path().join("README.md");
    let mut graphics = ReadingGraphics {
        document_path: Some(&document_path),
        style: None,
        native_heading: false,
        remaining_bytes: MAX_PREPARED_MEDIA_BYTES,
        remaining_renders: 2,
        prepared: HashMap::new(),
    };
    assert!(
        graphics
            .prepare(GraphicKind::Image, "later.png".into())
            .is_none()
    );
    std::fs::write(
        temp.path().join("later.png"),
        include_bytes!("../../../fixtures/editor/red-blue.png"),
    )
    .unwrap();
    assert!(
        graphics
            .prepare(GraphicKind::Image, "later.png".into())
            .is_none()
    );
    assert_eq!(graphics.remaining_renders, 1);
}

#[test]
fn early_text_keeps_copy_ranges_and_only_requests_supported_media() {
    let style = ReadingMediaStyle {
        foreground: [240; 3],
        background: [20; 3],
        cell_size: [8, 16],
    };
    let path = Path::new("README.md");
    let has_media = |source, style, native_heading| {
        editor_reading_text_blocks(
            Some(path),
            &[],
            &projection(source),
            style,
            native_heading,
            || false,
        )
        .1
    };
    assert!(!has_media(
        "# Heading\n\n![Remote](https://example.test/picture.png)",
        Some(style),
        true
    ));
    assert!(has_media("# Heading", Some(style), false));
    assert!(!has_media("$x^2$", None, false));
    assert!(has_media("$x^2$", Some(style), true));

    let (blocks, wants_assets) = editor_reading_text_blocks(
        Some(path),
        &[],
        &projection("![Label](local.png) $x^2$"),
        None,
        false,
        || false,
    );
    assert!(wants_assets);
    let [EditorRenderBlock::Paragraph(spans)] = blocks.as_slice() else {
        panic!("expected fallback paragraph");
    };
    for label in ["Label", "x^2"] {
        let span = spans.iter().find(|span| span.text == label).unwrap();
        assert!(span.media.is_none());
        assert!(span.rich_range.is_some());
    }
}
