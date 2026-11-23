use ascii_assets::{CELL_ICON_FONT, CELL_ICON_FRAME_MS, CellIconMode};
use serde_json::Value;
use std::collections::BTreeSet;
use ttf_parser::OutlineBuilder;

const PIXEL: f32 = 64.0;

#[derive(Default)]
struct PixelOutline {
    start: (f32, f32),
    previous: (f32, f32),
    vertical_edges: Vec<(f32, f32, f32)>,
}

impl PixelOutline {
    fn contains(&self, x: f32, y: f32) -> bool {
        self.vertical_edges
            .iter()
            .filter(|&&(edge_x, top, bottom)| x < edge_x && bottom < y && y < top)
            .count()
            % 2
            == 1
    }
}

impl OutlineBuilder for PixelOutline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.start = (x, y);
        self.previous = self.start;
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let (previous_x, previous_y) = self.previous;
        assert_eq!(x % PIXEL, 0.0);
        assert_eq!(y % PIXEL, 0.0);
        if previous_y != y {
            assert_eq!(previous_x, x);
            self.vertical_edges
                .push((x, previous_y.max(y), previous_y.min(y)));
        }
        self.previous = (x, y);
    }

    fn quad_to(&mut self, _: f32, _: f32, _: f32, _: f32) {
        panic!("cell icon outlines must follow the pixel grid");
    }

    fn curve_to(&mut self, _: f32, _: f32, _: f32, _: f32, _: f32, _: f32) {
        panic!("cell icon outlines must follow the pixel grid");
    }

    fn close(&mut self) {
        self.line_to(self.start.0, self.start.1);
    }
}

#[test]
fn every_catalog_role_selects_its_bundled_glyph_and_one_printable_ascii_character() {
    let pack: Value =
        serde_json::from_str(include_str!("../assets/cell-icons/icons.json")).unwrap();
    let face = ttf_parser::Face::parse(CELL_ICON_FONT, 0).unwrap();
    let glyphs = pack["glyphs"].as_object().unwrap();
    let groups = pack["groups"].as_object().unwrap();
    let has_name = |id, value| {
        face.names()
            .into_iter()
            .any(|name| name.name_id == id && name.to_string().as_deref() == Some(value))
    };
    assert!(has_name(ttf_parser::name_id::FAMILY, "Tundra Cell Atlas"));
    assert!(has_name(ttf_parser::name_id::VERSION, "Version 2.000"));
    assert_eq!(face.units_per_em(), 1152);
    assert_eq!(face.ascender(), 896);
    assert_eq!(face.descender(), -256);
    let mut points = BTreeSet::new();
    let mut glyph_ids = BTreeSet::new();
    for (name, glyph) in glyphs {
        let point = u32::from_str_radix(&glyph["codepoint"].as_str().unwrap()[2..], 16).unwrap();
        assert!(points.insert(point));
        let id = face.glyph_index(char::from_u32(point).unwrap()).unwrap();
        assert!(
            glyph_ids.insert(id.0),
            "{name} maps to a glyph already used"
        );
        assert_eq!(face.glyph_hor_advance(id), Some(576), "{name}");

        let mut outline = PixelOutline::default();
        let bounds = face.outline_glyph(id, &mut outline).unwrap();
        assert!(
            bounds.x_min >= 0 && bounds.x_max <= 576,
            "{name} exceeds the design columns"
        );
        assert!(
            bounds.y_min >= face.descender() && bounds.y_max <= face.ascender(),
            "{name} exceeds the design rows"
        );
        let rows = glyph["pixels"].as_array().unwrap();
        assert_eq!(rows.len(), 18, "{name}");
        for (row, expected) in rows.iter().enumerate() {
            let y = f32::from(face.ascender()) - (row as f32 + 0.5) * PIXEL;
            let actual: String = (0..9)
                .map(|column| {
                    if outline.contains((column as f32 + 0.5) * PIXEL, y) {
                        '#'
                    } else {
                        '.'
                    }
                })
                .collect();
            assert_eq!(actual, expected.as_str().unwrap(), "{name} row {}", row + 1);
        }
    }
    assert_eq!(points, (0x10f100..=0x10f187).collect());
    for old in 0xe100..=0xe187 {
        assert_eq!(face.glyph_index(char::from_u32(old).unwrap()), None);
    }
    assert_eq!(groups.len(), 8);
    let mut count = 0;
    for (group, entries) in groups {
        for (role, entry) in entries["roles"].as_object().unwrap() {
            let glyph = &glyphs[entry["glyph"].as_str().unwrap()];
            let font = CellIconMode::Font.icon(group, role);
            assert_eq!(font.chars().count(), 1);
            assert_eq!(
                format!("U+{:04X}", font.chars().next().unwrap() as u32),
                glyph["codepoint"]
            );
            let ascii = CellIconMode::Ascii.icon(group, role);
            assert!(matches!(ascii.as_bytes(), [b'!'..=b'~']));
            assert_eq!(ascii, glyph["ascii"]);
            count += 1;
        }
    }
    assert_eq!(count, 142);
    assert_eq!(CellIconMode::Ascii.icon("fileops", "delete"), "X");
    assert_eq!(CELL_ICON_FRAME_MS, 80);
    assert_eq!(CellIconMode::Font.busy_icon(9, false), "\u{10f174}");
    assert_eq!(CellIconMode::Font.busy_icon(10, false), "\u{10f16b}");
    assert_eq!(CellIconMode::Font.busy_icon(3, true), "\u{10f129}");
    assert_eq!(CellIconMode::Ascii.busy_icon(3, true), ".");
}
