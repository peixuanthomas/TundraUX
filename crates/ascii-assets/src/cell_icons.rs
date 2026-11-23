/// The bundled font installs independently of the active artwork theme.
pub const CELL_ICON_FONT: &[u8] = include_bytes!("../assets/cell-icons/TundraCellAtlas.ttf");

type Symbols = (&'static str, &'static str);
const UNKNOWN: Symbols = ("?", "?");
include!(concat!(env!("OUT_DIR"), "/cell_icons.rs"));

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum CellIconMode {
    #[default]
    Ascii,
    Font,
}

impl CellIconMode {
    pub fn icon(self, group: &str, role: &str) -> &'static str {
        self.select(lookup(group, role))
    }

    pub fn busy_icon(self, frame: u128, reduced_motion: bool) -> &'static str {
        self.select(if reduced_motion {
            STILL
        } else {
            FRAMES[(frame % FRAMES.len() as u128) as usize]
        })
    }

    fn select(self, (font, ascii): Symbols) -> &'static str {
        match self {
            Self::Ascii => ascii,
            Self::Font => font,
        }
    }
}
