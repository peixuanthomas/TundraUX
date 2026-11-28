use super::{Hello, LayoutGeneration, NativePath, WorkerId};
use base64_simd::{AsOut, STANDARD};
use compact_str::CompactString;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;

mod cell_encoding;
mod cell_reuse;

pub use cell_reuse::CellFrameCache;

/// Dimensions in terminal cells.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Size {
    pub width: u16,
    pub height: u16,
}

impl Size {
    pub const fn new(width: u16, height: u16) -> Self {
        Self { width, height }
    }

    pub const fn area(self) -> usize {
        self.width as usize * self.height as usize
    }
}

pub const MIN_CONTENT_SIZE: Size = Size::new(106, 16);

pub const MAX_IMAGE_BYTES: usize = 80_000_000;

/// Zero-based window-local coordinates and dimensions in terminal cells.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

impl Rect {
    pub const fn contains(self, x: u16, y: u16) -> bool {
        x >= self.x && y >= self.y && x - self.x < self.width && y - self.y < self.height
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShellToApp {
    Hello(Hello),
    Init(WindowInit),
    Viewport {
        layout: LayoutGeneration,
        /// `None` suspends viewport updates when the terminal cannot fit the workspace.
        size: Option<Size>,
        visible: bool,
        focused: bool,
    },
    Input {
        serial: u64,
        layout: LayoutGeneration,
        event: Value,
    },
    InputFence {
        serial: u64,
    },
    TaskAction {
        id: String,
        cancel: bool,
    },
    TaskCatalog {
        ids: Vec<String>,
    },
    FilePickerResult {
        request: u64,
        path: Option<NativePath>,
    },
    Back {
        layout: LayoutGeneration,
        action: BackAction,
    },
    Keyboard {
        layout: LayoutGeneration,
    },
    Help {
        layout: LayoutGeneration,
    },
    CancelPointer {
        layout: LayoutGeneration,
    },
    DismissNotificationReports {
        ids: Vec<u64>,
    },
    RequestClose,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppToShell {
    Welcome { worker: WorkerId },
    Frame(WindowFrame),
    InputFence { serial: u64 },
    TaskAction { id: String, cancel: bool },
    Status(WindowStatus),
    NotificationReport(Value),
    ToggleTasks,
    OpenApp(Value),
    FilePicked(NativePath),
    LaunchFailed(String),
    CloseRequested,
    CloseDeferred,
    CloseStarted,
    CloseCancelled,
    Closed,
    ShellAction(ShellAction),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShellAction {
    SettingsChanged,
    AutomationChanged(AutomationCommit),
    DebugPanicRequested,
    LanguageReloaded { code: String },
    ClockChanged,
    Restart,
    Reset(crate::ResetTarget),
    Exit,
    PowerOff,
    Reboot,
    UpdatePrepared(#[serde(with = "super::native_path")] PathBuf),
}

/// A persisted external-control setting, scoped to its authorization revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomationCommit {
    pub enabled: bool,
    pub revision: u64,
    pub authorization_revision: u64,
    pub authorization_session: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowInit {
    pub launch: Value,
    pub profile: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<Arc<Value>>,
    pub size: Size,
    pub layout: LayoutGeneration,
    pub focused: bool,
    #[serde(default)]
    pub image_cell_size: Option<[u16; 2]>,
    #[serde(default)]
    pub text_sizing: bool,
    #[serde(default)]
    pub ansi_colors: bool,
}

impl WindowInit {
    pub fn update_viewport(&mut self, message: &ShellToApp) {
        if let ShellToApp::Viewport {
            layout,
            size,
            focused,
            ..
        } = message
            && *layout >= self.layout
        {
            self.layout = *layout;
            self.size = size.unwrap_or(self.size);
            self.focused = *focused;
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowStatus {
    pub minimum: Size,
    pub busy: bool,
    pub dirty: bool,
    #[serde(default)]
    pub running_operations: bool,
    #[serde(default)]
    pub awaiting_approval: bool,
    #[serde(default)]
    pub unresponsive: bool,
    #[serde(default)]
    pub task_ids: Vec<String>,
}

impl Default for WindowStatus {
    fn default() -> Self {
        Self {
            minimum: MIN_CONTENT_SIZE,
            busy: false,
            dirty: false,
            running_operations: false,
            awaiting_approval: false,
            unresponsive: false,
            task_ids: Vec::new(),
        }
    }
}

/// AppHost-local monotonic durations, attached only to its first frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppStartupTimings {
    pub services_setup_us: u64,
    pub resources_us: u64,
    pub session_init_us: u64,
    #[serde(default)]
    pub session_build_us: u64,
    #[serde(default)]
    pub language_load_us: u64,
    #[serde(default)]
    pub launch_activate_us: u64,
    pub session_sync_us: u64,
    pub first_frame_ready_us: u64,
    pub first_poll_us: u64,
    pub first_draw_us: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackAction {
    Cancel,
    PreviousLocation,
    PreviousPage,
    Home,
    PowerMenu,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowFrame {
    pub sequence: u64,
    pub processed_input_serial: u64,
    #[serde(default)]
    pub confirmed_input_fence: u64,
    pub layout: LayoutGeneration,
    pub size: Size,
    #[serde(default)]
    pub page: String,
    #[serde(default)]
    pub animation_running: bool,
    #[serde(default)]
    pub keyboard_available: bool,
    #[serde(default)]
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_report: Option<u64>,
    /// A restored frame contains one cell per viewport position in row-major order.
    #[serde(with = "cell_encoding")]
    pub cells: Arc<Vec<Cell>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cell_reuse: Option<cell_reuse::CellReuse>,
    pub cursor: Option<Cursor>,
    pub images: Vec<Image>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_scroll: Option<ImageScroll>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headings: Vec<Heading>,
    pub controls: Vec<ControlNode>,
    pub masks: Vec<Rect>,
    pub native_escape: Vec<Rect>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub back_action: Option<BackAction>,
    #[serde(default)]
    pub ctrl_w_passthrough: bool,
    /// Vim uses Ctrl+Q for native visual-block mode.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ctrl_q_passthrough: bool,
    pub modal: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub startup_timings: Option<AppStartupTimings>,
}

impl WindowFrame {
    pub fn new(layout: LayoutGeneration, size: Size) -> Self {
        Self::with_cells(layout, size, vec![Cell::default(); size.area()])
    }

    /// Builds a frame from cells already captured in row-major order.
    pub fn with_cells(layout: LayoutGeneration, size: Size, cells: Vec<Cell>) -> Self {
        Self {
            sequence: 0,
            processed_input_serial: 0,
            confirmed_input_fence: 0,
            layout,
            size,
            page: String::new(),
            animation_running: false,
            keyboard_available: false,
            status: String::new(),
            status_report: None,
            cells: Arc::new(cells),
            cell_reuse: None,
            cursor: None,
            images: Vec::new(),
            image_scroll: None,
            headings: Vec::new(),
            controls: Vec::new(),
            masks: Vec::new(),
            native_escape: Vec::new(),
            back_action: None,
            ctrl_w_passthrough: false,
            ctrl_q_passthrough: false,
            modal: false,
            startup_timings: None,
        }
    }

    pub fn validate(&self) -> io::Result<()> {
        if self.cell_reuse.is_some() || self.cells.len() != self.size.area() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid cell count",
            ));
        }
        if self
            .cells
            .iter()
            .any(|cell| cell.symbol.contains(char::is_control))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid control character",
            ));
        }
        if self
            .cursor
            .is_some_and(|cursor| cursor.x >= self.size.width || cursor.y >= self.size.height)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "cursor outside window",
            ));
        }
        if self.headings.iter().any(|heading| {
            let rect = heading.area;
            rect.width == 0
                || rect.height != 2
                || !rect
                    .x
                    .checked_add(rect.width)
                    .is_some_and(|end| end <= self.size.width)
                || !rect
                    .y
                    .checked_add(rect.height)
                    .is_some_and(|end| end <= self.size.height)
                || !(1..=6).contains(&heading.level)
                || heading.text.contains(char::is_control)
        }) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid heading",
            ));
        }
        if self.image_scroll.is_some_and(|scroll| {
            scroll.height == 0
                || scroll
                    .top
                    .checked_add(scroll.height)
                    .is_none_or(|end| end > self.size.height)
        }) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "image scroll outside window",
            ));
        }
        for image in &self.images {
            let pixels = u64::from(image.width) * u64::from(image.height);
            if pixels == 0
                || pixels > (MAX_IMAGE_BYTES / 4) as u64
                || (!image.rgba.is_empty()
                    && pixels.checked_mul(4) != Some(image.rgba.len() as u64))
                || image.rows.is_some_and(|rows| {
                    image.preview_id.is_none()
                        || rows.total == 0
                        || rows
                            .first
                            .checked_add(image.area.height)
                            .is_none_or(|end| end > rows.total)
                })
            {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid RGBA image",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Cell {
    #[serde(default = "blank", skip_serializing_if = "is_blank")]
    pub symbol: CompactString,
    #[serde(default, skip_serializing_if = "Color::is_reset")]
    pub foreground: Color,
    #[serde(default, skip_serializing_if = "Color::is_reset")]
    pub background: Color,
    #[serde(default, skip_serializing_if = "Color::is_reset")]
    pub underline_color: Color,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub modifiers: u16,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub skip: bool,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            symbol: blank(),
            foreground: Color::Reset,
            background: Color::Reset,
            underline_color: Color::Reset,
            modifiers: 0,
            skip: false,
        }
    }
}

fn blank() -> CompactString {
    " ".into()
}

fn is_blank(symbol: &str) -> bool {
    symbol == " "
}

fn is_zero(modifiers: &u16) -> bool {
    *modifiers == 0
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Color {
    #[default]
    Reset,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

impl Color {
    fn is_reset(&self) -> bool {
        *self == Self::Reset
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cursor {
    pub x: u16,
    pub y: u16,
    pub shape: CursorShape,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CursorShape {
    #[default]
    Default,
    BlinkingBlock,
    SteadyBlock,
    BlinkingUnderline,
    SteadyUnderline,
    BlinkingBar,
    SteadyBar,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Image {
    pub image_id: u64,
    /// Identifies the same editor picture or PDF page across changes of raster and crop.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview_id: Option<u64>,
    /// Cell rows of a fitted rich-text graphic before viewport clipping.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rows: Option<ImageRows>,
    pub width: u32,
    pub height: u32,
    #[serde(
        serialize_with = "serialize_pixels",
        deserialize_with = "deserialize_pixels"
    )]
    /// Pixels for a new image ID. Empty when an earlier frame supplied them.
    pub rgba: Arc<[u8]>,
    pub source: [u32; 4],
    pub padding: [u16; 4],
    pub cell_size: [u16; 2],
    pub area: Rect,
    pub visible: Vec<Rect>,
    pub z_index: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageRows {
    pub first: u16,
    pub total: u16,
}

/// Pane-local canvas rows and the document row displayed at the top.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageScroll {
    pub top: u16,
    pub height: u16,
    pub first_line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Heading {
    pub area: Rect,
    pub text: String,
    pub level: u8,
    pub foreground: Color,
}

fn serialize_pixels<S: serde::Serializer>(pixels: &[u8], serializer: S) -> Result<S::Ok, S::Error> {
    if pixels.is_empty() {
        return serializer.serialize_str("");
    }
    serializer.collect_str(&std::fmt::from_fn(|formatter| {
        // Complete base64 groups let us stream large images without a second full-size buffer.
        let mut encoded = [0; 16 * 1024];
        for chunk in pixels.chunks(encoded.len() / 4 * 3) {
            formatter.write_str(STANDARD.encode_as_str(chunk, encoded.as_mut_slice().as_out()))?;
        }
        Ok(())
    }))
}

fn deserialize_pixels<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Arc<[u8]>, D::Error> {
    #[derive(Deserialize)]
    struct Encoded<'a>(#[serde(borrow)] std::borrow::Cow<'a, str>);
    let Encoded(encoded) = Encoded::deserialize(deserializer)?;
    let bytes = encoded.as_bytes();
    let length = STANDARD
        .decoded_length(bytes)
        .map_err(serde::de::Error::custom)?;
    let mut pixels = bytemuck::allocation::zeroed_arc_slice(length);
    STANDARD
        .decode(bytes, Arc::get_mut(&mut pixels).unwrap().as_out())
        .map_err(serde::de::Error::custom)?;
    Ok(pixels)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlNode {
    pub id: String,
    pub parent: Option<String>,
    pub role: String,
    pub name: String,
    pub text: String,
    pub rect: Rect,
    pub focused: bool,
    pub selected: bool,
    pub enabled: bool,
    pub actionable: bool,
    pub sensitive: bool,
    pub actions: Vec<String>,
}
