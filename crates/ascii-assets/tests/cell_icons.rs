use ascii_assets::{CELL_ICON_FONT, CELL_ICON_FRAME_MS, CellIconMode};
use serde_json::Value;
use std::collections::BTreeSet;

#[test]
fn every_catalog_role_selects_its_bundled_glyph_and_one_printable_ascii_character() {
    let pack: Value =
        serde_json::from_str(include_str!("../assets/cell-icons/icons.json")).unwrap();
    let face = ttf_parser::Face::parse(CELL_ICON_FONT, 0).unwrap();
    let glyphs = pack["glyphs"].as_object().unwrap();
    let groups = pack["groups"].as_object().unwrap();
    let mut points = BTreeSet::new();
    for glyph in glyphs.values() {
        let point = u32::from_str_radix(&glyph["codepoint"].as_str().unwrap()[2..], 16).unwrap();
        assert!(points.insert(point));
        let id = face.glyph_index(char::from_u32(point).unwrap()).unwrap();
        assert_eq!(face.glyph_hor_advance(id), Some(576));
        assert!(face.glyph_bounding_box(id).is_some());
    }
    assert_eq!(points, (0xe100..=0xe187).collect());
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
    assert_eq!(CellIconMode::Font.busy_icon(9, false), "\u{e174}");
    assert_eq!(CellIconMode::Font.busy_icon(10, false), "\u{e16b}");
    assert_eq!(CellIconMode::Font.busy_icon(3, true), "\u{e129}");
    assert_eq!(CellIconMode::Ascii.busy_icon(3, true), ".");
}
