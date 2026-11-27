use super::document::*;
use super::*;

mod markdown_preview_graphics {
    use super::*;
    use crate::{RenderContext, TerminalImageCache};
    use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
    use ratatui::{Terminal, backend::TestBackend};
    use std::io::Cursor;

    fn graphic(id: u64, width: u32, height: u32) -> EditorPreviewGraphic {
        let mut bytes = Cursor::new(Vec::new());
        let pixels = RgbaImage::from_pixel(width, height, Rgba([200, 150, 100, 255]));
        DynamicImage::ImageRgba8(pixels)
            .write_to(&mut bytes, ImageFormat::Png)
            .unwrap();
        EditorPreviewGraphic {
            id,
            image: app::editor::PreviewImage::decode(bytes.get_ref())
                .unwrap()
                .into(),
        }
    }

    fn reading(blocks: Vec<EditorRenderBlock>) -> EditorViewModel {
        let mut model = EditorViewModel::new("reading.md", blocks);
        model.markdown_reading = Some(true);
        model.image_protocol = EditorImageProtocolStatus::Available;
        model.image_cell_size = [8, 16];
        model
    }

    #[test]
    fn inline_math_wraps_as_one_item_and_reserves_its_height() {
        let mut formula =
            EditorRenderSpan::plain("x+y").with_rich_range(RichRange::from_raw(4, 3, 4));
        formula.media = Some(graphic(1, 32, 32));
        let model = reading(vec![EditorRenderBlock::Paragraph(vec![
            EditorRenderSpan::plain("abc"),
            formula,
            EditorRenderSpan::plain("TAIL"),
        ])]);
        let full = flatten_rich_document(&model, 6, 8);
        let measured = ReadOnlyRichLayout::measure(&model, 6, 8);
        assert_eq!(measured.line_count(), 4);
        assert_eq!(measured.materialize(&model, 0..4), full);
        let text = |line: usize| {
            full[line]
                .runs
                .iter()
                .map(|run| run.text.resolve(None))
                .collect::<String>()
        };
        assert_eq!(text(0), "abc");
        assert_eq!(text(1), "x+yTA");
        assert_eq!(text(2), "  ");
        assert_eq!(text(3), "IL");
        for row in &full[1..3] {
            let boundaries = display_line_rich_boundaries(row, None, 0, 6);
            assert_eq!(
                boundaries
                    .iter()
                    .map(|boundary| boundary.column)
                    .collect::<Vec<_>>(),
                [0, 4]
            );
        }
    }

    #[test]
    fn tall_inline_math_in_table_crops_in_both_directions_inside_split_pane() {
        let mut formula = EditorRenderSpan::plain("x+y");
        formula.media = Some(graphic(2, 48, 48));
        let table_id = NodeId::new(15);
        let mut blocks = vec![EditorRenderBlock::RichTable {
            table_id,
            header: vec![
                EditorTableCell::text("left"),
                EditorTableCell::text("right"),
            ],
            rows: vec![vec![
                EditorTableCell {
                    spans: vec![formula],
                },
                EditorTableCell::text("other"),
            ]],
            alignments: vec![EditorTableAlignment::Left, EditorTableAlignment::Left],
        }];
        blocks.extend(std::iter::repeat_n(EditorRenderBlock::Blank, 20));
        let mut model = reading(blocks);
        model.rich_table_column_widths.insert(table_id, vec![8, 8]);
        model.horizontal_scroll = 4;
        model.scroll_line = 4;
        let area = Rect::new(4, 3, 12, 9);
        let mut terminal = Terminal::new(TestBackend::new(24, 18)).unwrap();
        let context = RenderContext::from_theme(
            &TundraTheme::default_dark(),
            Default::default(),
            Default::default(),
        );
        let mut images = TerminalImageCache::default();
        images.capture_workspace(area);
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
                assert_eq!(layout.visible_start, 4);
                assert_eq!(layout.horizontal_scroll, 4);
                assert!(layout.visible_capacity >= 2);
            })
            .unwrap();
        let images = images.take_workspace_images();
        let [image] = images.as_slice() else {
            panic!("expected a single clipped formula")
        };
        assert_eq!(image.preview_id, Some(2));
        assert_eq!(image.area.x, 0);
        assert_eq!(image.area.width, 4);
        assert_eq!(image.area.height, 2);
        assert_eq!(image.source, [16, 16, 32, 32]);
    }

    #[test]
    fn selected_formula_falls_back_to_readable_source_and_does_not_cover_selection() {
        let range = RichRange::from_raw(5, 2, 3);
        let mut formula = EditorRenderSpan::plain("x+y").with_rich_range(range);
        formula.media = Some(graphic(3, 32, 16));
        let mut model = reading(vec![EditorRenderBlock::Paragraph(vec![
            EditorRenderSpan::plain("a "),
            formula,
            EditorRenderSpan::plain(" z"),
        ])]);
        model.rich_selection = Some(range);
        let area = Rect::new(0, 0, 28, 10);
        let mut terminal = Terminal::new(TestBackend::new(28, 10)).unwrap();
        let theme = TundraTheme::default_dark();
        let context = RenderContext::from_theme(&theme, Default::default(), Default::default());
        let mut images = TerminalImageCache::default();
        images.capture_workspace(area);
        let layout = editor_layout(area, &model);
        terminal
            .draw(|frame| {
                render_editor_contextual_with_terminal_images(
                    frame,
                    area,
                    &model,
                    &context,
                    &mut images,
                    None,
                );
            })
            .unwrap();
        assert!(images.take_workspace_images().is_empty());
        let buffer = terminal.backend().buffer();
        let cell = |column| {
            buffer
                .cell((layout.canvas.x + column, layout.canvas.y))
                .unwrap()
        };
        assert_eq!(
            [cell(2).symbol(), cell(3).symbol(), cell(4).symbol()],
            ["x", "+", "y"]
        );
        assert_ne!(cell(1).bg, cell(2).bg);
        assert_eq!(cell(7).symbol(), "z");
    }

    #[test]
    fn text_only_graphics_use_source_but_keep_local_pictures() {
        let mut formula = EditorRenderSpan::plain("abc");
        formula.media = Some(graphic(4, 32, 32));
        let blocks = vec![
            EditorRenderBlock::Paragraph(vec![formula]),
            EditorRenderBlock::PreviewGraphic {
                graphic: graphic(5, 32, 48),
                kind: EditorPreviewGraphicKind::Mermaid,
                fallback: "a --> b\nb --> c".into(),
                range: None,
            },
            EditorRenderBlock::PreviewGraphic {
                graphic: graphic(6, 32, 48),
                kind: EditorPreviewGraphicKind::Picture,
                fallback: "local photo".into(),
                range: None,
            },
        ];
        let mut model = reading(blocks);
        model.image_protocol = EditorImageProtocolStatus::Unsupported;
        let lines = flatten_rich_document(&model, 20, 12);
        assert_eq!(lines.len(), 6);
        let text = |line: usize| {
            lines[line]
                .runs
                .iter()
                .map(|run| run.text.resolve(None))
                .collect::<String>()
        };
        assert_eq!(text(0), "abc");
        assert_eq!(text(1), "a --> b");
        assert_eq!(text(2), "b --> c");
        assert!(matches!(&lines[3].runs[0].text, DisplayText::Graphic(graphic) if graphic.picture));
    }

    #[test]
    fn heading_prefers_native_text_then_graphic_then_plain_text() {
        let block = EditorRenderBlock::PreviewHeading {
            level: 2,
            spans: vec![
                EditorRenderSpan::plain("Example").with_rich_range(RichRange::from_raw(7, 0, 7)),
            ],
            graphic: graphic(7, 48, 48),
        };
        let mut model = reading(vec![block]);
        model.rich_cursor = Some(RichPosition::in_node(NodeId::new(7), 0));
        model.text_sizing_protocol = true;
        let lines = flatten_rich_document(&model, 40, 8);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].role, DisplayLineRole::HeadingTop(2));
        model.text_sizing_protocol = false;
        let lines = flatten_rich_document(&model, 40, 8);
        assert_eq!(lines.len(), 3);
        assert!(matches!(lines[0].runs[0].text, DisplayText::Graphic(_)));
        model.image_protocol = EditorImageProtocolStatus::Unsupported;
        let lines = flatten_rich_document(&model, 40, 8);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].runs.iter().all(|run| run.style.bold));
        assert_eq!(
            lines[0]
                .runs
                .iter()
                .map(|run| run.text.resolve(None))
                .collect::<String>(),
            "Example"
        );
    }

    #[test]
    fn code_tabs_use_terminal_columns_without_changing_selection_offsets() {
        let block = EditorRenderBlock::RichCodeBlock {
            language: Some("sh".into()),
            lines: vec![
                EditorRenderSpan::plain("a\tz").with_rich_range(RichRange::from_raw(8, 0, 3)),
            ],
        };
        let model = reading(vec![block]);
        let lines = flatten_rich_document(&model, 20, 8);
        assert_eq!(runs_width(&lines[1].runs), 7);
        let positions = display_line_rich_boundaries(&lines[1], None, 0, 20)
            .into_iter()
            .filter(|boundary| boundary.editable)
            .map(|boundary| (boundary.column, boundary.position.grapheme_offset))
            .collect::<Vec<_>>();
        assert_eq!(positions, [(2, 0), (3, 1), (6, 2), (7, 3)]);
    }

    #[test]
    fn cropped_markdown_picture_reuses_fitted_pixels_after_returning_from_source() {
        let graphic = graphic(9, 768, 576);
        let original = graphic.image.rgba.clone();
        let mut blocks = vec![
            EditorRenderBlock::paragraph("before"),
            EditorRenderBlock::PreviewGraphic {
                graphic,
                kind: EditorPreviewGraphicKind::Picture,
                fallback: "local picture".into(),
                range: None,
            },
        ];
        blocks.extend(std::iter::repeat_n(EditorRenderBlock::Blank, 50));
        let mut model = reading(blocks);
        model.scroll_line = 2;
        let theme = TundraTheme::default_dark();
        let context = RenderContext::from_theme(&theme, Default::default(), Default::default());
        let mut terminal = Terminal::new(TestBackend::new(132, 59)).unwrap();
        let mut images = TerminalImageCache::default();
        let mut capture = |model: &EditorViewModel, area| {
            images.capture_workspace(area);
            terminal
                .draw(|frame| {
                    render_editor_contextual_with_terminal_images(
                        frame,
                        area,
                        model,
                        &context,
                        &mut images,
                        None,
                    );
                })
                .unwrap();
            images.take_workspace_images()
        };
        let compact = Rect::new(4, 3, 64, 9);
        let first = capture(&model, compact).pop().unwrap();
        assert_eq!(first.preview_id, Some(9));
        assert_eq!([first.width, first.height], [152, 112]);
        assert_eq!(first.source, [0, 16, 152, 96]);
        assert_eq!(&first.rgba[..4], &[200, 150, 100, 255]);
        assert!(first.rgba.len() < original.len() / 20);

        let source = EditorViewModel::source("reading.md", "raw Markdown");
        assert!(capture(&source, compact).is_empty());
        let reopened = capture(&model, compact).pop().unwrap();
        assert_eq!(reopened.image_id, first.image_id);
        assert!(Arc::ptr_eq(&reopened.rgba, &first.rgba));

        let enlarged = capture(&model, Rect::new(4, 3, 120, 52)).pop().unwrap();
        assert_eq!(enlarged.preview_id, first.preview_id);
        assert_eq!(enlarged.image_id, 9);
        assert_eq!([enlarged.width, enlarged.height], [768, 576]);
        assert!(Arc::ptr_eq(&enlarged.rgba, &original));
    }
}

#[cfg(test)]
mod rich_layout_measurement_tests {
    use super::*;

    fn mapped(text: &str) -> EditorRenderSpan {
        EditorRenderSpan::plain(text).with_rich_range(RichRange::from_raw(
            42,
            0,
            Span::raw(text).styled_graphemes(Style::default()).count(),
        ))
    }

    fn representative_blocks() -> Vec<EditorRenderBlock> {
        vec![
            EditorRenderBlock::Paragraph(Vec::new()),
            EditorRenderBlock::Paragraph(vec![EditorRenderSpan::plain("")]),
            EditorRenderBlock::Paragraph(vec![EditorRenderSpan::plain(
                "plain 好 e\u{301} 🙂 text that wraps",
            )]),
            EditorRenderBlock::Paragraph(vec![mapped("first\r\nsecond\rthird\n")]),
            EditorRenderBlock::Paragraph(vec![
                EditorRenderSpan::plain("first\n"),
                EditorRenderSpan::plain("\u{1b}"),
            ]),
            EditorRenderBlock::Paragraph(vec![
                mapped("好"),
                mapped("plain span following a wide grapheme"),
            ]),
            EditorRenderBlock::Heading {
                level: 2,
                spans: vec![mapped("a heading that wraps")],
            },
            EditorRenderBlock::BulletListItem {
                depth: 2,
                checked: Some(false),
                spans: vec![mapped("task item with a continuation")],
            },
            EditorRenderBlock::OrderedListItem {
                depth: 1,
                number: 123,
                spans: vec![mapped("ordered item")],
            },
            EditorRenderBlock::Quote {
                depth: 2,
                spans: vec![mapped("quoted text\nwith a break")],
            },
            EditorRenderBlock::Footnote {
                label: "note".to_string(),
                spans: vec![mapped("footnote body")],
            },
            EditorRenderBlock::CodeBlock {
                language: Some("rust".to_string()),
                lines: vec!["fn main() {}".to_string(), "// line 2".to_string()],
            },
            EditorRenderBlock::RichCodeBlock {
                language: Some("text".into()),
                lines: vec![
                    EditorRenderSpan::plain("a\tfoo")
                        .with_rich_range(RichRange::from_raw(66, 0, 5)),
                    EditorRenderSpan::plain("bar").with_rich_range(RichRange::from_raw(66, 6, 9)),
                ],
            },
            EditorRenderBlock::Table {
                header: vec![EditorTableCell::text("A"), EditorTableCell::text("B")],
                rows: vec![vec![
                    EditorTableCell::text("one"),
                    EditorTableCell::text("two"),
                ]],
                alignments: vec![EditorTableAlignment::Left, EditorTableAlignment::Right],
            },
            EditorRenderBlock::Table {
                header: Vec::new(),
                rows: Vec::new(),
                alignments: Vec::new(),
            },
            EditorRenderBlock::HorizontalRule,
            EditorRenderBlock::RawHtml("<div>raw\nhtml</div>".to_string()),
            EditorRenderBlock::Image {
                markdown: "![preview](image.png)".to_string(),
            },
            EditorRenderBlock::Blank,
        ]
    }

    #[test]
    fn bounded_measurement_matches_materialized_block_line_counts() {
        for block in representative_blocks() {
            for width in 1..=24 {
                let expected = block_lines(&block, 0, width, None, None, None).len();
                for limit in 0..=expected.saturating_add(1) {
                    assert_eq!(
                        rich_block_line_count_up_to(&block, width, limit),
                        (expected <= limit).then_some(expected),
                        "block={block:?}, width={width}, limit={limit}"
                    );
                }
            }
        }
    }

    #[test]
    fn multiline_raw_markdown_uses_structural_display_lines() {
        let block = EditorRenderBlock::RawHtml("first\r\nsecond\rthird\n".to_string());

        let lines = block_lines(&block, 0, 80, None, None, None);
        let rendered = lines
            .iter()
            .map(|line| {
                line.runs
                    .iter()
                    .map(|run| run.text.resolve(None))
                    .collect::<String>()
            })
            .collect::<Vec<_>>();

        assert_eq!(rendered, ["HTML first", "second", "third", ""]);
        assert!(lines.iter().all(|line| {
            line.runs
                .iter()
                .all(|run| !run.text.resolve(None).contains(['\r', '\n']))
        }));
    }

    #[test]
    fn rich_scrollbar_layout_flattens_the_document_only_once() {
        let blocks: Arc<[EditorRenderBlock]> = (0..10_000)
            .map(|index| EditorRenderBlock::paragraph(format!("line {index}")))
            .collect::<Vec<_>>()
            .into();
        let mut model = EditorViewModel::new_shared("large.md", blocks);
        model.scroll_line = 5_000;

        RICH_FLATTEN_CALL_COUNT.with(|count| count.set(0));
        let layout = editor_layout(Rect::new(0, 0, 80, 12), &model);
        let flatten_calls = RICH_FLATTEN_CALL_COUNT.with(std::cell::Cell::get);

        assert_eq!(flatten_calls, 1);
        assert_eq!(layout.document_line_count, 10_000);
        assert_eq!(layout.visible_start, 5_000);
        assert!(layout.vertical_scrollbar.is_some());
        assert_eq!(layout.prepared_lines.len(), layout.visible_capacity);
    }

    #[test]
    fn read_only_rich_measurements_and_window_runs_match_the_full_layout() {
        let table_id = NodeId::new(900);
        let mut blocks = representative_blocks();
        blocks.splice(
            2..2,
            [
                EditorRenderBlock::RichTable {
                    table_id,
                    header: vec![EditorTableCell {
                        spans: vec![mapped("heading 栏目")],
                    }],
                    rows: vec![
                        vec![
                            EditorTableCell {
                                spans: vec![mapped("value spans multiple cells 好 and wraps")],
                            },
                            EditorTableCell {
                                spans: vec![mapped("another\r\nthen more")],
                            },
                        ],
                        Vec::new(),
                        vec![EditorTableCell {
                            spans: vec![mapped("tail")],
                        }],
                    ],
                    alignments: vec![EditorTableAlignment::Center, EditorTableAlignment::Right],
                },
                EditorRenderBlock::RawHtml("line 好\r\nlong emoji e\u{301} 🤦‍♀️\u{1b}\n".to_string()),
                EditorRenderBlock::Image {
                    markdown: "an image\r\nwith a soft line break".to_string(),
                },
                EditorRenderBlock::CodeBlock {
                    language: Some("short".to_string()),
                    lines: vec!["a long line of code \u{202e} 好".to_string()],
                },
            ],
        );
        let mut model = EditorViewModel::new("document.docx", blocks);
        model.read_only = true;
        model.rich_table_column_widths.insert(table_id, vec![8, 5]);
        for big_text in [false, true] {
            model.text_sizing_protocol = big_text;
            for width in [1, 14, 33, 84] {
                let full = flatten_rich_document(&model, width, 12);
                let measured = ReadOnlyRichLayout::measure(&model, width, 12);
                assert_eq!(measured.line_count(), full.len());
                assert_eq!(
                    measured.content_width,
                    rich_horizontal_content_width(&full, model.source.as_deref())
                );
                for start in 0..full.len() {
                    let end = (start + 4).min(full.len());
                    assert_eq!(measured.materialize(&model, start..end), full[start..end]);
                }
            }
        }
    }

    #[test]
    fn read_only_rich_layout_reaches_the_bottom_of_ten_thousand_paragraphs_without_flattening() {
        let blocks = (0..10_000)
            .map(|index| EditorRenderBlock::paragraph(format!("paragraph {index:05}")))
            .collect();
        let mut model = EditorViewModel::new("long.docx", blocks);
        model.read_only = true;
        let area = Rect::new(1, 2, 120, 36);
        RICH_FLATTEN_CALL_COUNT.with(|count| count.set(0));
        for requested in [0, 5_000, usize::MAX] {
            model.scroll_line = requested;
            let layout = editor_layout(area, &model);
            assert_eq!(layout.document_line_count, 10_000);
            assert_eq!(
                layout.visible_start,
                requested.min(10_000 - layout.visible_capacity)
            );
            assert_eq!(layout.prepared_lines.len(), layout.visible_capacity);
            assert_eq!(
                layout.prepared_lines[0].block_index,
                Some(layout.visible_start)
            );
            assert!(layout.vertical_scrollbar.is_some());
            assert!(layout.horizontal_scrollbar.is_none());
        }
        assert_eq!(RICH_FLATTEN_CALL_COUNT.with(std::cell::Cell::get), 0);
    }

    #[test]
    fn prepared_read_only_layout_preserves_scroll_and_ignores_stale_geometry() {
        let blocks = (0..1_000)
            .map(|row| EditorRenderBlock::paragraph(format!("row {row} 好 e\u{301}")))
            .collect();
        let mut direct = EditorViewModel::new("long.docx", blocks);
        direct.read_only = true;
        let area = Rect::new(1, 2, 120, 36);
        let key =
            EditorReadOnlyLayoutKey::for_area(area, None, direct.image_cell_size, false, None);
        let prepared = EditorReadOnlyLayout::prepare(key, &direct, || false).unwrap();
        let mut cached = direct.clone();
        cached.read_only_layout = Some(EditorReadOnlyLayoutState::Ready(Arc::new(prepared)));

        for top in [0, 500, usize::MAX] {
            direct.scroll_line = top;
            cached.scroll_line = top;
            let expected = editor_layout(area, &direct);
            READ_ONLY_MEASURE_CALLS.set(0);
            assert_eq!(editor_layout(area, &cached), expected);
            assert_eq!(READ_ONLY_MEASURE_CALLS.get(), 0);
        }

        let resized = editor_layout(Rect { width: 108, ..area }, &cached);
        assert!(resized.read_only_layout_pending);
        assert!(resized.prepared_lines.is_empty());
        assert!(
            resized
                .hit_test(resized.canvas.x, resized.canvas.y)
                .is_none()
        );
        cached.image_cell_size = [11, 23];
        assert!(editor_layout(area, &cached).read_only_layout_pending);
    }

    #[test]
    fn read_only_layout_preparation_can_stop_during_a_large_table() {
        let table = EditorRenderBlock::Table {
            header: vec![EditorTableCell::text("header")],
            rows: vec![vec![EditorTableCell::text("value")]; 1_000],
            alignments: Vec::new(),
        };
        let mut model = EditorViewModel::new("large.xlsx", vec![table]);
        model.read_only = true;
        let key = EditorReadOnlyLayoutKey::for_area(
            Rect::new(1, 2, 120, 36),
            None,
            model.image_cell_size,
            false,
            None,
        );
        let checks = std::cell::Cell::new(0);
        assert!(
            EditorReadOnlyLayout::prepare(key, &model, || {
                checks.set(checks.get() + 1);
                checks.get() >= 20
            })
            .is_none()
        );
        assert_eq!(checks.get(), 20);
    }

    #[test]
    fn read_only_sheet_scrolls_into_a_wrapped_row_and_keeps_its_rich_hit() {
        let rows = (0..10_000)
            .map(|index| {
                vec![
                    EditorTableCell {
                        spans: vec![
                            EditorRenderSpan::plain("a".repeat(60))
                                .with_rich_range(RichRange::from_raw(100 + index, 0, 60)),
                        ],
                    },
                    EditorTableCell::text("wide second column"),
                ]
            })
            .collect();
        let table = EditorRenderBlock::RichTable {
            table_id: NodeId::new(10),
            header: vec![
                EditorTableCell::text("Name"),
                EditorTableCell::text("Details"),
            ],
            rows,
            alignments: vec![EditorTableAlignment::Left, EditorTableAlignment::Right],
        };
        let mut model = EditorViewModel::new("large.xlsx", vec![table]);
        model.read_only = true;
        model.horizontal_scroll = 6;
        model.scroll_line = 3 + 3 * 5_000 + 1;
        RICH_FLATTEN_CALL_COUNT.with(|count| count.set(0));
        let layout = editor_layout(Rect::new(1, 1, 40, 16), &model);
        assert_eq!(RICH_FLATTEN_CALL_COUNT.with(std::cell::Cell::get), 0);
        assert_eq!(layout.document_line_count, 30_004);
        assert_eq!(layout.visible_start, model.scroll_line);
        assert_eq!(layout.horizontal_scroll, 6);
        assert!(layout.vertical_scrollbar.is_some());
        assert!(layout.horizontal_scrollbar.is_some());
        let hit = layout
            .hit_test_document(layout.canvas.x + 4, layout.canvas.y)
            .unwrap();
        let EditorDocumentPosition::Rich(position) = hit.position else {
            panic!("sheet must expose the visible table cell's Rich text");
        };
        assert_eq!(position.container_id, NodeId::new(5_100));
        assert!((24..48).contains(&position.grapheme_offset));
        assert!(hit.editable);
        model.scroll_line = usize::MAX;
        let bottom = editor_layout(Rect::new(1, 1, 40, 16), &model);
        assert_eq!(bottom.visible_start + bottom.visible_capacity, 30_004);
        assert!(
            bottom
                .prepared_lines
                .last()
                .unwrap()
                .runs
                .iter()
                .any(|run| run.text.resolve(None) == "┘")
        );
    }

    #[test]
    fn scrollbar_measurement_preserves_the_wide_layout_fixed_point() {
        // At full width this is exactly one line. Reserving a scrollbar first
        // would wrap it and incorrectly make the scrollbar self-fulfilling.
        let fitting =
            EditorViewModel::new("fit.md", vec![EditorRenderBlock::paragraph("x".repeat(22))]);
        let fitting_layout = editor_layout(Rect::new(0, 0, 22, 3), &fitting);
        assert_eq!(fitting_layout.canvas.width, 22);
        assert_eq!(fitting_layout.visible_capacity, 1);
        assert_eq!(fitting_layout.document_line_count, 1);
        assert!(fitting_layout.vertical_scrollbar.is_none());

        let overflowing = EditorViewModel::new(
            "overflow.md",
            vec![EditorRenderBlock::paragraph("x".repeat(45))],
        );
        let overflowing_layout = editor_layout(Rect::new(0, 0, 22, 3), &overflowing);
        assert_eq!(overflowing_layout.canvas.width, 21);
        assert_eq!(overflowing_layout.document_line_count, 3);
        assert!(overflowing_layout.vertical_scrollbar.is_some());
    }
}

#[cfg(test)]
mod rich_table_identity_tests {
    use super::*;

    fn rich_table(table_id: NodeId) -> EditorRenderBlock {
        EditorRenderBlock::RichTable {
            table_id,
            header: vec![
                EditorTableCell {
                    spans: vec![
                        EditorRenderSpan::plain("Name")
                            .with_rich_range(RichRange::from_raw(101, 0, 4)),
                    ],
                },
                EditorTableCell {
                    spans: vec![
                        EditorRenderSpan::plain("Value")
                            .with_rich_range(RichRange::from_raw(102, 0, 5)),
                    ],
                },
            ],
            rows: vec![vec![
                EditorTableCell {
                    spans: vec![
                        EditorRenderSpan::plain("Tundra")
                            .with_rich_range(RichRange::from_raw(201, 0, 6)),
                    ],
                },
                EditorTableCell {
                    spans: vec![
                        EditorRenderSpan::plain("3")
                            .with_rich_range(RichRange::from_raw(202, 0, 1)),
                    ],
                },
            ]],
            alignments: vec![EditorTableAlignment::Left, EditorTableAlignment::Right],
        }
    }

    #[test]
    fn rich_table_handles_and_widths_follow_stable_table_id() {
        let table_id = NodeId::new(9001);
        let mut model = EditorViewModel::new(
            "table.md",
            vec![EditorRenderBlock::paragraph("before"), rich_table(table_id)],
        );
        model.rich_table_column_widths.insert(table_id, vec![9, 4]);

        let first_layout = editor_layout(Rect::new(0, 0, 80, 16), &model);
        let first_resize = first_layout
            .table_resize_handles
            .iter()
            .find(|handle| handle.table_id == Some(table_id) && handle.column_index == 0)
            .expect("Rich table resize handle");
        assert_eq!(first_resize.block_index, 1);
        assert_eq!(first_resize.width, 9);
        assert_eq!(
            first_layout.hit_test(first_resize.area.x, first_resize.area.y + 1),
            Some(EditorHitTarget::RichTableResize {
                table_id,
                column_index: 0,
                width: 9,
            })
        );

        let left_edge = first_layout
            .table_edge_handles
            .iter()
            .find(|handle| {
                handle.table_id == Some(table_id) && handle.edge == EditorTableEdge::Left
            })
            .expect("Rich table edge handle");
        assert_eq!(left_edge.source_range, None);
        assert_eq!(
            first_layout.hit_test(left_edge.area.x, left_edge.area.y + 1),
            Some(EditorHitTarget::RichTableEdge {
                table_id,
                edge: EditorTableEdge::Left,
            })
        );

        model
            .blocks
            .insert(0, EditorRenderBlock::paragraph("new leading block"));
        let shifted_layout = editor_layout(Rect::new(0, 0, 80, 18), &model);
        let shifted_resize = shifted_layout
            .table_resize_handles
            .iter()
            .find(|handle| handle.table_id == Some(table_id) && handle.column_index == 0)
            .expect("shifted Rich table resize handle");
        assert_eq!(shifted_resize.block_index, 2);
        assert_eq!(shifted_resize.width, 9);
    }
}

#[cfg(test)]
mod rich_newline_position_tests {
    use super::*;

    #[test]
    fn soft_break_second_line_hit_is_the_boundary_after_the_break() {
        let container = NodeId::new(77);
        let model = EditorViewModel::new(
            "soft-break.md",
            vec![EditorRenderBlock::Paragraph(vec![
                EditorRenderSpan::plain("a ").with_rich_range(RichRange::from_raw(
                    container.get(),
                    0,
                    2,
                )),
                EditorRenderSpan::plain("\n").with_rich_range(RichRange::from_raw(
                    container.get(),
                    2,
                    3,
                )),
            ])],
        );
        let layout = editor_layout(Rect::new(0, 0, 40, 10), &model);
        let after_break = RichPosition::in_node(container, 3);

        let visual = layout
            .visual_position_for_rich(after_break)
            .expect("soft-break boundary has a visual position");
        assert_eq!(visual, EditorTextPosition::new(1, 0));

        let hit = layout
            .hit_test_document(
                layout.canvas.x.saturating_add(to_u16(visual.column)),
                layout
                    .canvas
                    .y
                    .saturating_add(to_u16(visual.line.saturating_sub(layout.visible_start))),
            )
            .expect("second visual line is hittable");
        assert_eq!(hit.position, EditorDocumentPosition::Rich(after_break));
        assert!(hit.editable);
        assert_eq!(
            layout.visual_position_for_document(hit.position),
            Some(visual)
        );
    }

    #[test]
    fn source_newline_mapping_remains_byte_based() {
        let model = EditorViewModel::source("source.md", "a \nb");
        let layout = editor_layout(Rect::new(0, 0, 40, 10), &model);
        let after_newline = 3;
        let visual = layout
            .visual_position_for_source(after_newline)
            .expect("source byte boundary has a visual position");
        assert_eq!(visual, EditorTextPosition::new(1, 0));

        let hit = layout
            .hit_test_document(
                layout.canvas.x.saturating_add(to_u16(visual.column)),
                layout
                    .canvas
                    .y
                    .saturating_add(to_u16(visual.line.saturating_sub(layout.visible_start))),
            )
            .expect("second source line is hittable");
        assert_eq!(hit.position, EditorDocumentPosition::Source(after_newline));
        assert!(hit.editable);
    }
}

#[cfg(test)]
mod shared_rich_blocks_tests {
    use super::*;

    #[test]
    fn shared_projection_is_authoritative_and_reused_by_layout() {
        let blocks: Arc<[EditorRenderBlock]> = vec![
            EditorRenderBlock::paragraph("shared paragraph"),
            EditorRenderBlock::Image {
                markdown: "![shared](preview.png)".to_string(),
            },
        ]
        .into();
        let retained = Arc::clone(&blocks);
        let model = EditorViewModel::new_shared("shared.md", blocks);

        assert!(model.blocks.is_empty());
        assert!(Arc::ptr_eq(
            model.shared_blocks.as_ref().expect("shared projection"),
            &retained
        ));
        assert_eq!(model.render_blocks().len(), 2);
        let layout = editor_layout(Rect::new(0, 0, 80, 14), &model);
        assert_eq!(layout.document_line_count, 2);
        assert_eq!(layout.image_areas.len(), 1);
        assert_eq!(layout.image_areas[0].block_index, 1);
    }
}

#[cfg(test)]
mod source_virtualization_tests {
    use super::*;

    #[test]
    fn coalesced_source_run_preserves_unicode_byte_boundaries() {
        let source = "A好e\u{301}🙂";
        let model = EditorViewModel::source("unicode.log", source);
        let layout = editor_layout(Rect::new(0, 0, 60, 10), &model);

        assert_eq!(layout.prepared_lines.len(), 1);
        assert_eq!(layout.prepared_lines[0].runs.len(), 1);
        assert_eq!(
            layout.prepared_lines[0].runs[0].text,
            DisplayText::SourceRange(EditorSourceRange::new(0, source.len()))
        );
        let boundaries = &layout.source_line_maps[0].boundaries;
        assert!(
            boundaries
                .iter()
                .any(|boundary| { boundary.column == 3 && boundary.byte_offset == "A好".len() })
        );
        assert!(boundaries.iter().any(|boundary| {
            boundary.column == 4 && boundary.byte_offset == "A好e\u{301}".len()
        }));
        assert!(layout.rich_line_maps.is_empty());
    }

    #[test]
    fn long_source_line_prepares_one_run_and_bounded_hit_boundaries() {
        let source = "x".repeat(1_000_000);
        let model = EditorViewModel::source("single-line.log", &source);
        let layout = editor_layout(Rect::new(0, 0, 80, 12), &model);

        assert_eq!(layout.document_line_count, 1);
        assert_eq!(layout.prepared_lines.len(), 1);
        assert_eq!(layout.prepared_lines[0].runs.len(), 1);
        assert!(
            layout.source_line_maps[0].boundaries.len()
                <= usize::from(layout.canvas.width).saturating_add(2),
            "{} boundaries for canvas width {}",
            layout.source_line_maps[0].boundaries.len(),
            layout.canvas.width
        );
    }

    #[test]
    fn many_source_lines_prepare_only_the_scrolled_viewport() {
        let source = (0..100_000)
            .map(|line| format!("line {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut model = EditorViewModel::source("many-lines.log", source);
        model.scroll_line = 50_000;
        let layout = editor_layout(Rect::new(0, 0, 80, 12), &model);

        assert_eq!(layout.document_line_count, 100_000);
        assert_eq!(layout.visible_start, 50_000);
        assert_eq!(layout.prepared_lines.len(), layout.visible_capacity);
        assert!(layout.prepared_lines.len() < 100_000);
        assert_eq!(layout.source_line_maps.len(), layout.visible_capacity);
        assert!(
            layout
                .prepared_lines
                .iter()
                .all(|line| line.runs.len() == 1)
        );
        assert_eq!(
            layout
                .source_line_maps
                .first()
                .map(|line| line.document_line),
            Some(50_000)
        );
        assert!(layout.rich_line_maps.is_empty());
    }

    #[test]
    fn viewport_only_source_keeps_global_lines_columns_and_byte_offsets() {
        let first_line = 50_000;
        let first_byte = 900_000;
        let lines = (0..32)
            .map(|relative| {
                let text = if relative == 0 { "好x" } else { "row" };
                let start = first_byte + relative * 16;
                EditorSourceWindowLine::new(
                    EditorSourceRange::new(start, start + text.len()),
                    200,
                    text,
                )
            })
            .collect();
        let mut model = EditorViewModel::source_viewport("window.log", first_line, 100_000, lines);
        model.horizontal_scroll = 200;
        model.horizontal_content_width = 400;
        let layout = editor_layout(Rect::new(0, 0, 80, 12), &model);

        assert_eq!(layout.document_line_count, 100_000);
        assert_eq!(layout.visible_start, first_line);
        assert_eq!(layout.prepared_lines.len(), layout.visible_capacity);
        assert_eq!(layout.prepared_lines[0].column_start, 200);
        assert!(matches!(
            layout.prepared_lines[0].runs[0].text,
            DisplayText::Shared(_)
        ));
        let hit = layout
            .hit_test_source(layout.canvas.x + 2, layout.canvas.y)
            .expect("window line remains globally addressable");
        assert_eq!(hit.visual, EditorTextPosition::new(first_line, 202));
        assert_eq!(hit.byte_offset, first_byte + "好".len());
    }
}
