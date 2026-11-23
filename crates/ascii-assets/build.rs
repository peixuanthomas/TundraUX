use serde::Deserialize;
use std::collections::BTreeMap;
use std::fmt::Write;
use std::path::PathBuf;

#[derive(Deserialize)]
struct Pack {
    glyphs: BTreeMap<String, Glyph>,
    groups: BTreeMap<String, Group>,
    bindings: BTreeMap<String, BTreeMap<String, String>>,
    busy_indicator: Animation,
}

#[derive(Deserialize)]
struct Glyph {
    codepoint: String,
    ascii: char,
}

impl Glyph {
    fn symbols(&self) -> (String, String) {
        let code = u32::from_str_radix(self.codepoint.strip_prefix("U+").unwrap(), 16).unwrap();
        assert!(self.ascii.is_ascii_graphic());
        (
            char::from_u32(code).unwrap().to_string(),
            self.ascii.to_string(),
        )
    }
}

#[derive(Deserialize)]
struct Group {
    roles: BTreeMap<String, Role>,
}

#[derive(Deserialize)]
struct Role {
    glyph: String,
}

#[derive(Deserialize)]
struct Animation {
    frames: Vec<String>,
    frame_ms: u64,
    reduced_motion: String,
}

fn main() {
    let path = "assets/cell-icons/icons.json";
    println!("cargo:rerun-if-changed={path}");
    let pack: Pack = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let mut groups = pack.bindings;
    for (name, group) in pack.groups {
        let roles = group
            .roles
            .into_iter()
            .map(|(role, entry)| (role, entry.glyph))
            .collect();
        assert!(groups.insert(name, roles).is_none());
    }

    // App windows open in separate processes. Compile the lookup once, including ASCII fallbacks.
    let mut code = String::from("fn lookup(group: &str, role: &str) -> Symbols { match group {\n");
    for (group, roles) in groups {
        writeln!(code, "{group:?} => match role {{").unwrap();
        for (role, glyph) in roles {
            writeln!(code, "{role:?} => {:?},", pack.glyphs[&glyph].symbols()).unwrap();
        }
        code.push_str("_ => UNKNOWN, },\n");
    }
    code.push_str("_ => UNKNOWN, } }\n");
    let animation = pack.busy_indicator;
    let frames: Vec<_> = animation
        .frames
        .iter()
        .map(|key| pack.glyphs[key].symbols())
        .collect();
    assert!(!frames.is_empty() && animation.frame_ms > 0);
    writeln!(code, "const FRAMES: &[Symbols] = &{frames:?};").unwrap();
    writeln!(
        code,
        "pub const CELL_ICON_FRAME_MS: u64 = {};",
        animation.frame_ms
    )
    .unwrap();
    writeln!(
        code,
        "const STILL: Symbols = {:?};",
        pack.glyphs[&animation.reduced_motion].symbols()
    )
    .unwrap();
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    std::fs::write(out.join("cell_icons.rs"), code).unwrap();
}
