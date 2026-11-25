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

fn preview(document: &PdfPreviewDocument, page: usize, area: Rect) -> EditorViewModel {
    let mut model = EditorViewModel::new("测试 e\u{301}.PDF", Vec::new());
    model.read_only = true;
    model.cursor = None;
    model.pdf = Some(EditorPdfViewModel {
        page,
        total: document.page_count(),
        focus: EditorPdfAction::Open,
        image: Some((
            1,
            EditorPdfPageImage::prepare(
                document
                    .render_page(page, [area.width, area.height - 2], [8, 16])
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
            let model = preview(&document, 0, area);
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
fn pdf_preview_transfers_only_the_fitted_page_to_all_native_backends() {
    let document = PdfPreviewDocument::open(SAMPLE.to_vec()).unwrap();
    let area = Rect::new(2, 3, 106, 16);
    let model = preview(&document, 1, area);
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
