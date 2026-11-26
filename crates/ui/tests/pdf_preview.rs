use std::path::Path;
use std::sync::Arc;

use app::editor::PdfPreviewDocument;
use ratatui::{Terminal, backend::TestBackend, layout::Rect, style::Color};
use ui::{
    ColorCapability, EditorGraphicsProtocol, EditorPdfAction, EditorPdfPageImage,
    EditorPdfViewModel, EditorViewModel, RenderCapabilities, RenderContext, TerminalGraphicsProbe,
    TerminalImageCache, TundraTheme, editor_layout, render_editor_contextual_with_terminal_images,
};

const SAMPLE: &[u8] = include_bytes!("../../app/tests/fixtures/pdf/preview.pdf");

fn preview(
    document: &PdfPreviewDocument,
    page: usize,
    area: Rect,
    cell_size: [u16; 2],
) -> EditorViewModel {
    let mut model = EditorViewModel::new("测试 e\u{301}.PDF", Vec::new());
    model.read_only = true;
    model.cursor = None;
    model.pdf = Some(EditorPdfViewModel {
        page,
        page_id: 42,
        total: document.page_count(),
        focus: EditorPdfAction::Open,
        zoom: Default::default(),
        jump: None,
        image_zoom: None,
        image: Some((
            1,
            EditorPdfPageImage::prepare(
                document
                    .render_page(page, [area.width, area.height - 2], cell_size)
                    .unwrap(),
            )
            .unwrap(),
        )),
        error: None,
    });
    model
}

#[test]
fn pdf_preview_fits_three_window_sizes_in_both_languages_and_color_modes() {
    let document = PdfPreviewDocument::open(SAMPLE.to_vec()).unwrap();
    let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("../ascii-assets/assets");
    for language in ["en-US", "zh-CN"] {
        let language = i18n::LanguageSnapshot::load(&assets, language, 1)
            .unwrap()
            .snapshot;
        let _language = i18n::enter_snapshot(Arc::new(language));
        for (width, height) in [(108, 20), (120, 36), (180, 50)] {
            let area = Rect::new(1, 2, width - 2, height - 4);
            let model = preview(&document, 0, area, [8, 16]);
            for color in [ColorCapability::TrueColor, ColorCapability::Ansi] {
                let context = RenderContext::from_theme(
                    &TundraTheme::default_dark(),
                    Default::default(),
                    RenderCapabilities {
                        color,
                        ..Default::default()
                    },
                );
                let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                let mut images = TerminalImageCache::default();
                terminal
                    .draw(|frame| {
                        let layout = render_editor_contextual_with_terminal_images(
                            frame,
                            area,
                            &model,
                            &context,
                            &mut images,
                            None,
                        );
                        assert_eq!(layout.canvas.height, area.height - 2);
                        let footer = layout.status_bar;
                        let controls = layout.pdf.unwrap();
                        let buttons = (footer.x..footer.right())
                            .filter_map(|x| controls.button_at((x, footer.y)))
                            .collect::<Vec<_>>();
                        assert!(
                            buttons
                                .iter()
                                .any(|button| button.id.as_str() == "editor.pdf.previous"
                                    && button.disabled)
                        );
                        assert!(
                            buttons
                                .iter()
                                .any(|button| button.id.as_str() == "editor.pdf.next"
                                    && !button.disabled)
                        );
                        for (id, disabled) in [
                            ("editor.pdf.jump", false),
                            ("editor.pdf.rotate", false),
                            ("editor.pdf.zoom-in", false),
                            ("editor.pdf.zoom-out", true),
                        ] {
                            assert!(
                                buttons.iter().any(|button| button.id.as_str() == id
                                    && button.disabled == disabled)
                            );
                        }
                    })
                    .unwrap();
                let buffer = terminal.backend().buffer();
                let footer = (area.x..area.right())
                    .map(|x| buffer[(x, area.bottom() - 1)].symbol())
                    .collect::<String>();
                assert!(footer.contains('1') && footer.contains('2'));
                let blocks = buffer
                    .content()
                    .iter()
                    .filter(|cell| cell.symbol() == "▀")
                    .collect::<Vec<_>>();
                assert!(!blocks.is_empty());
                assert!(blocks.iter().any(|cell| cell.bg != cell.fg));
                assert_eq!(
                    blocks.iter().any(|cell| matches!(cell.bg, Color::Rgb(..))),
                    color == ColorCapability::TrueColor
                );
            }
        }
    }
}

#[test]
fn magnified_pdf_crops_the_blocks_and_native_image_together() {
    let document = PdfPreviewDocument::open(SAMPLE.to_vec()).unwrap();
    let area = Rect::new(1, 2, 106, 16);
    let mut model = preview(&document, 1, area, [8, 16]);
    let canvas = editor_layout(area, &model).canvas;
    let pdf = model.pdf.as_mut().unwrap();
    for _ in 0..4 {
        pdf.zoom.change(true);
    }
    let raster = document
        .render_page(1, pdf.zoom.pdf_viewport(canvas), [8, 16])
        .unwrap();
    let size = raster.cells.map(u32::from);
    pdf.image = Some((2, EditorPdfPageImage::prepare(raster).unwrap()));
    let center = pdf.zoom.window(canvas, size);
    assert_eq!(center.offset, center.limit.map(|limit| limit / 2));
    assert!(center.offset.into_iter().all(|offset| offset > 0));

    let context = RenderContext::from_theme(
        &TundraTheme::default_dark(),
        Default::default(),
        Default::default(),
    );
    let mut terminal = Terminal::new(TestBackend::new(108, 20)).unwrap();
    let mut cache = TerminalImageCache::default();
    let mut draw = |model: &EditorViewModel| {
        cache.capture_workspace(area);
        terminal
            .draw(|frame| {
                render_editor_contextual_with_terminal_images(
                    frame, area, model, &context, &mut cache, None,
                );
            })
            .unwrap();
        let image = cache.take_workspace_images().pop().unwrap();
        (image, terminal.backend().buffer().clone())
    };
    let (first, blocks) = draw(&model);
    model.pdf.as_mut().unwrap().zoom.pan(center, [1, 1]);
    let (panned, shifted) = draw(&model);
    assert_eq!(panned.image_id, first.image_id);
    assert_eq!(panned.area, first.area);
    assert!(panned.source[0] > first.source[0] && panned.source[1] > first.source[1]);
    for y in canvas.y..canvas.bottom() - 1 {
        for x in canvas.x..canvas.right() - 1 {
            assert_eq!(shifted[(x, y)], blocks[(x + 1, y + 1)]);
        }
    }
}

#[test]
fn pdf_preview_transfers_only_the_fitted_page_to_all_native_backends() {
    let document = PdfPreviewDocument::open(SAMPLE.to_vec()).unwrap();
    let area = Rect::new(2, 3, 106, 16);
    let model = preview(&document, 1, area, [8, 16]);
    let context = RenderContext::from_theme(
        &TundraTheme::default_dark(),
        Default::default(),
        Default::default(),
    );
    let mut worker = Terminal::new(TestBackend::new(120, 24)).unwrap();
    let mut cache = TerminalImageCache::default();
    cache.capture_workspace(area);
    worker
        .draw(|frame| {
            render_editor_contextual_with_terminal_images(
                frame, area, &model, &context, &mut cache, None,
            );
        })
        .unwrap();
    let images = cache.take_workspace_images();
    assert_eq!(images.len(), 1);
    let position = images[0].area;
    assert_eq!(
        (position.x, position.y, position.width, position.height),
        (32, 1, 42, 14)
    );

    for (protocol, prefix) in [
        (EditorGraphicsProtocol::Kitty, "\x1b_G"),
        (EditorGraphicsProtocol::Sixel, "\x1bP"),
        (EditorGraphicsProtocol::Iterm2, "\x1b]1337;File=inline=1"),
    ] {
        let probe =
            TerminalGraphicsProbe::from_terminal_capabilities(protocol, 8, 16, false, false);
        let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();
        let mut images_cache = TerminalImageCache::default();
        terminal
            .draw(|frame| {
                images_cache.render_workspace(frame, area, &images, probe.picker().unwrap());
            })
            .unwrap();
        assert!(
            terminal.backend().buffer()[(area.x + position.x, area.y + position.y)]
                .symbol()
                .contains(prefix)
        );
    }

    let smaller = Rect::new(area.x, area.y, area.width, area.height - 2);
    cache.capture_workspace(smaller);
    worker
        .draw(|frame| {
            render_editor_contextual_with_terminal_images(
                frame, smaller, &model, &context, &mut cache, None,
            );
        })
        .unwrap();
    assert!(cache.take_workspace_images().is_empty());
    assert!(editor_layout(smaller, &model).canvas.height < images[0].area.height);
}

#[test]
fn large_pdf_preview_uses_iterm2_after_the_editor_pane_expands() {
    let document = PdfPreviewDocument::open(SAMPLE.to_vec()).unwrap();
    let context = RenderContext::from_theme(
        &TundraTheme::default_dark(),
        Default::default(),
        Default::default(),
    );
    let cell_size = [14, 32];
    let graphics = TerminalGraphicsProbe::from_terminal_capabilities(
        EditorGraphicsProtocol::Iterm2,
        cell_size[0],
        cell_size[1],
        false,
        false,
    );
    let mut worker = Terminal::new(TestBackend::new(208, 55)).unwrap();
    let mut host = Terminal::new(TestBackend::new(208, 55)).unwrap();
    let mut worker_cache = TerminalImageCache::default();
    let mut host_cache = TerminalImageCache::default();
    for height in [24, 51, 36] {
        let area = Rect::new(1, 2, 206, height);
        let model = preview(&document, 1, area, cell_size);
        worker_cache.capture_workspace(area);
        worker
            .draw(|frame| {
                render_editor_contextual_with_terminal_images(
                    frame,
                    area,
                    &model,
                    &context,
                    &mut worker_cache,
                    None,
                );
            })
            .unwrap();
        let images = worker_cache.take_workspace_images();
        assert_eq!(images.len(), 1);
        let image = &images[0];
        let origin = (area.x + image.area.x, area.y + image.area.y);
        assert!(
            !worker.backend().buffer()[origin]
                .symbol()
                .contains("\x1b]1337;")
        );
        assert!(matches!(
            worker.backend().buffer()[origin].bg,
            Color::Rgb(..)
        ));
        if height == 51 {
            assert!(image.width < u32::from(image.area.width) * u32::from(cell_size[0]));
        }
        host.draw(|frame| {
            host_cache.render_workspace(frame, area, &images, graphics.picker().unwrap());
        })
        .unwrap();
        let symbol = host.backend().buffer()[origin].symbol();
        assert!(symbol.contains("\x1b]1337;File=inline=1"));
        assert!(symbol.contains(&format!(
            "width={};height={};",
            image.area.width, image.area.height
        )));
    }
}
