use ratatui::layout::{HorizontalAlignment, Rect};

use crate::components::terminal_width;

pub const MIN_SHELL_TERMINAL_WIDTH: u16 = 108;
pub const MIN_SHELL_TERMINAL_HEIGHT: u16 = 20;

// Compact chrome is a rendering fallback; Shell enforces the terminal minimum.
const COMPACT_CHROME_WIDTH: u16 = 50;
const COMPACT_CHROME_HEIGHT: u16 = 12;
const MIN_STATUS_MESSAGE_WIDTH: u16 = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellLayout {
    Compact(Rect),
    Full { top: Rect, main: Rect, bottom: Rect },
}

pub fn compute_shell_layout(area: Rect) -> ShellLayout {
    if area.width < COMPACT_CHROME_WIDTH || area.height < COMPACT_CHROME_HEIGHT {
        let header_height = area.height.min(1);
        return ShellLayout::Compact(Rect::new(
            area.x,
            area.y.saturating_add(header_height),
            area.width,
            area.height.saturating_sub(header_height),
        ));
    }

    let top = Rect::new(area.x, area.y, area.width, 1);
    let main = Rect::new(
        area.x,
        area.y.saturating_add(1),
        area.width,
        area.height.saturating_sub(2),
    );
    let bottom = Rect::new(
        area.x,
        area.y.saturating_add(area.height.saturating_sub(1)),
        area.width,
        1,
    );

    ShellLayout::Full { top, main, bottom }
}
pub(crate) fn rect_contains(area: Rect, x: u16, y: u16) -> bool {
    area.width > 0
        && area.height > 0
        && x >= area.x
        && x < area.x.saturating_add(area.width)
        && y >= area.y
        && y < area.y.saturating_add(area.height)
}

pub(crate) fn inset_rect(area: Rect, margin: u16) -> Rect {
    let doubled = margin.saturating_mul(2);
    Rect::new(
        area.x.saturating_add(margin.min(area.width)),
        area.y.saturating_add(margin.min(area.height)),
        area.width.saturating_sub(doubled),
        area.height.saturating_sub(doubled),
    )
}

pub(crate) fn line_in_rect(area: Rect, y: u16) -> Rect {
    if area.width == 0 || area.height == 0 || y < area.y || y >= area.y.saturating_add(area.height)
    {
        return Rect::new(area.x, area.y.saturating_add(area.height), 0, 0);
    }
    Rect::new(area.x, y, area.width, 1)
}

pub(crate) fn usize_to_u16(value: usize) -> u16 {
    u16::try_from(value).unwrap_or(u16::MAX)
}

pub(crate) fn centered_rect(area: Rect, width: u16, height: u16) -> Rect {
    Rect::new(
        area.x.saturating_add(area.width.saturating_sub(width) / 2),
        area.y
            .saturating_add(area.height.saturating_sub(height) / 2),
        width,
        height,
    )
}

/// Geometry shared by composition, hit testing and effects, including keyboard
/// animation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShellFrameLayout {
    pub bounds: Rect,
    pub shell: ShellLayout,
    pub main: Rect,
    pub title: Rect,
    pub back_button: Option<Rect>,
    pub help_button: Option<Rect>,
    pub status_message: Option<Rect>,
    pub status_alignment: HorizontalAlignment,
    pub workspace_taskbar: Option<Rect>,
    pub time_button: Option<Rect>,
    pub keyboard_launcher: Option<Rect>,
    help_width: u16,
    time_width: u16,
    keyboard_width: u16,
}

impl ShellFrameLayout {
    pub fn new(bounds: Rect, time_label: Option<&str>, context: &crate::RenderContext) -> Self {
        let mut shell = compute_shell_layout(bounds);
        let main = match &mut shell {
            ShellLayout::Full { main, .. } => {
                *main = context.page_area(*main);
                *main
            }
            ShellLayout::Compact(main) => *main,
        };
        Self {
            bounds,
            shell,
            main,
            title: Rect::default(),
            back_button: None,
            help_button: None,
            status_message: None,
            status_alignment: HorizontalAlignment::Center,
            workspace_taskbar: None,
            time_button: None,
            keyboard_launcher: None,
            help_width: 0,
            time_width: time_label
                .filter(|label| !label.is_empty())
                .map_or(0, |label| {
                    usize_to_u16(terminal_width(label).saturating_add(2))
                }),
            keyboard_width: 0,
        }
        .with_back_label(None)
    }

    pub fn is_compact(self) -> bool {
        matches!(self.shell, ShellLayout::Compact(_))
    }

    /// The keyboard takes over the bottom row and clips the page above it.
    pub fn with_input_keyboard(mut self, keyboard: Rect, visibility: u16) -> Self {
        if visibility == 0 {
            return self;
        }
        match self.shell {
            ShellLayout::Full {
                top, mut bottom, ..
            } => {
                bottom.y = self.bounds.bottom();
                bottom.height = 0;
                let page_bottom = keyboard.y.min(self.bounds.bottom());
                self.main.height = page_bottom.saturating_sub(self.main.y);
                self.shell = ShellLayout::Full {
                    top,
                    main: self.main,
                    bottom,
                };
                self.workspace_taskbar = None;
                self.layout_header();
            }
            ShellLayout::Compact(_) => {
                self.main.height = keyboard
                    .y
                    .min(self.bounds.bottom())
                    .saturating_sub(self.main.y);
                self.shell = ShellLayout::Compact(self.main);
                self.keyboard_launcher = None;
            }
        }
        self
    }

    /// The keyboard entry follows the status message in the header.
    pub fn with_keyboard_launcher(mut self, available: bool) -> Self {
        if !available || self.keyboard_launcher.is_some() {
            return self;
        }
        if self.is_compact() {
            if !self.main.is_empty() {
                self.keyboard_launcher = Some(crate::screen_keyboard_launcher_area(self.main));
                self.main.height = self.main.height.saturating_sub(1);
                self.shell = ShellLayout::Compact(self.main);
            }
        } else {
            self.keyboard_width = crate::screen_keyboard_launcher_area(self.bounds).width;
            self.layout_header();
        }
        self
    }

    pub fn with_back_label(mut self, label: Option<&str>) -> Self {
        self.back_button = label
            .filter(|_| !self.bounds.is_empty())
            .map(|label| {
                let icon = terminal_width(crate::assets::BACK_ICON.trim());
                let width = usize_to_u16(terminal_width(label).saturating_add(icon + 3)).min(
                    self.bounds
                        .width
                        .saturating_sub(usize_to_u16(terminal_width(&i18n::tr!("ui-shell-title")))),
                );
                Rect::new(self.bounds.right() - width, self.bounds.y, width, 1)
            })
            .filter(|area| !area.is_empty());
        self.layout_header();
        self
    }

    pub fn with_help_button(mut self, available: bool) -> Self {
        self.help_width = if available && !self.is_compact() {
            usize_to_u16(terminal_width(&i18n::tr!("ui-shell-help-button")).saturating_add(2))
        } else {
            0
        };
        self.layout_header();
        self
    }

    pub fn with_workspace_taskbar(mut self, available: bool) -> Self {
        self.workspace_taskbar = match self.shell {
            ShellLayout::Full { bottom, .. } if available && !bottom.is_empty() => Some(bottom),
            _ => None,
        };
        self
    }

    /// Leave chrome and the expanded keyboard accessible when the page is clipped.
    pub fn modal_area(self) -> Rect {
        match self.shell {
            ShellLayout::Full { bottom, .. } if !bottom.is_empty() => self.bounds,
            _ => self.main,
        }
    }

    fn layout_header(&mut self) {
        let right = self.back_button.map_or(self.bounds.right(), |back| back.x);
        let mut area = Rect::new(
            self.bounds.x,
            self.bounds.y,
            right - self.bounds.x,
            self.bounds.height.min(1),
        );
        let title_width =
            usize_to_u16(terminal_width(&i18n::tr!("ui-shell-title"))).min(area.width);
        self.title = Rect::new(area.x, area.y, title_width, area.height);
        let title_space = title_width.saturating_add(1).min(area.width);
        area.x += title_space;
        area.width -= title_space;
        let ShellLayout::Full { bottom, .. } = self.shell else {
            self.status_message = (!area.is_empty()).then_some(area);
            return;
        };

        let time_width = self
            .time_width
            .min(area.width.saturating_sub(MIN_STATUS_MESSAGE_WIDTH + 1));
        self.time_button = take_header_button(&mut area, time_width);
        self.help_button = take_header_button(&mut area, self.help_width);
        self.keyboard_launcher = if bottom.is_empty() {
            None
        } else {
            let width = self.keyboard_width.min(area.width.saturating_sub(2));
            take_header_button(&mut area, width)
        };
        self.status_message = (!area.is_empty()).then_some(area);
    }
}

fn take_header_button(area: &mut Rect, width: u16) -> Option<Rect> {
    let width = width.min(area.width.saturating_sub(1));
    if width == 0 || area.is_empty() {
        return None;
    }
    area.width -= width + 1;
    Some(Rect::new(area.right(), area.y, width, area.height))
}

/// PTY viewport on a standalone terminal, with the physical size limit.
pub fn command_line_terminal_area(area: Rect) -> Option<Rect> {
    if area.width < crate::MIN_COMMAND_LINE_TERMINAL_WIDTH
        || area.height < crate::MIN_COMMAND_LINE_TERMINAL_HEIGHT
    {
        return None;
    }
    let layout = ShellFrameLayout::new(area, None, &crate::RenderContext::default());
    command_line_terminal_area_in(&layout)
}

/// PTY and scrollbar geometry in the compositor, including AppHost virtual layouts.
pub fn command_line_terminal_area_in(layout: &ShellFrameLayout) -> Option<Rect> {
    let minimum = workspace_protocol::MIN_CONTENT_SIZE;
    (!layout.is_compact()
        && layout.bounds.width >= minimum.width
        && layout.bounds.height >= minimum.height + 2
        && !layout.main.is_empty())
    .then_some(layout.main)
}
