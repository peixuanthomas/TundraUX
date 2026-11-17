use ratatui::Frame;
use ratatui::buffer::Buffer;
use ratatui::layout::{Margin, Rect};

use crate::RenderContext;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ScrollbarOrientation {
    #[default]
    VerticalRight,
    HorizontalBottom,
}

impl ScrollbarOrientation {
    /// Extends pointer hits by one cell on each side without extending the ends.
    pub fn hit_test(self, area: Rect, point: (u16, u16)) -> bool {
        let margin = match self {
            Self::VerticalRight => Margin::new(1, 0),
            Self::HorizontalBottom => Margin::new(0, 1),
        };
        !area.is_empty() && area.outer(margin).contains(point.into())
    }
}

/// A token-aware scrollbar. The thumb geometry is entirely derived from the
/// current area, so callers can rebuild hit maps from the same values each
/// animation frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scrollbar {
    pub content_len: usize,
    pub viewport_len: usize,
    pub offset: usize,
    pub orientation: ScrollbarOrientation,
}

impl Default for Scrollbar {
    fn default() -> Self {
        Self {
            content_len: 0,
            viewport_len: 0,
            offset: 0,
            orientation: ScrollbarOrientation::VerticalRight,
        }
    }
}

impl Scrollbar {
    pub const fn new(content_len: usize, viewport_len: usize, offset: usize) -> Self {
        Self {
            content_len,
            viewport_len,
            offset,
            orientation: ScrollbarOrientation::VerticalRight,
        }
    }

    pub const fn orientation(mut self, orientation: ScrollbarOrientation) -> Self {
        self.orientation = orientation;
        self
    }

    pub fn thumb_range(&self, area: Rect) -> (u16, u16) {
        let track_len = match self.orientation {
            ScrollbarOrientation::VerticalRight => area.height,
            ScrollbarOrientation::HorizontalBottom => area.width,
        };
        if track_len == 0 || self.content_len <= self.viewport_len || self.content_len == 0 {
            return (0, 0);
        }
        let thumb_len = ((track_len as usize * self.viewport_len).div_ceil(self.content_len))
            .max(1)
            // Keep room to drag even when only one content row is hidden.
            .min(usize::from(track_len.saturating_sub(1).max(1))) as u16;
        let max_offset = self.content_len.saturating_sub(self.viewport_len).max(1);
        let start = (((track_len.saturating_sub(thumb_len) as usize) * self.offset.min(max_offset))
            / max_offset) as u16;
        (start, thumb_len)
    }

    pub fn render(&self, area: Rect, buffer: &mut Buffer, context: &RenderContext) {
        let tokens = context.theme;
        let (start, len) = self.thumb_range(area);
        if area.is_empty() || len == 0 {
            return;
        }
        // Half the width and a quarter of the height look equally thick in a 1:2 cell.
        let (id, track, thumb, track_symbol, thumb_symbol) = match self.orientation {
            ScrollbarOrientation::VerticalRight => {
                let x = area.right() - 1;
                (
                    "scrollbar.vertical",
                    Rect::new(x, area.y, 1, area.height),
                    Rect::new(x, area.y + start, 1, len),
                    "▎",
                    "▌",
                )
            }
            ScrollbarOrientation::HorizontalBottom => {
                let y = area.bottom() - 1;
                (
                    "scrollbar.horizontal",
                    Rect::new(area.x, y, area.width, 1),
                    Rect::new(area.x + start, y, len, 1),
                    "▁",
                    "▂",
                )
            }
        };
        if crate::automation::enabled() {
            let _scope = crate::automation::scope(id, "group", "", track);
            crate::automation::control(
                "track",
                "scrollbar",
                "",
                track,
                Default::default(),
                &["click", "scroll"],
            );
            crate::automation::control("thumb", "slider", "", thumb, Default::default(), &["drag"]);
        }
        for point in track.intersection(buffer.area).positions() {
            let (symbol, color) = if thumb.contains(point) {
                (thumb_symbol, tokens.accent_strong)
            } else {
                (track_symbol, tokens.border)
            };
            buffer[point]
                .set_symbol(symbol)
                .set_fg(color)
                .set_bg(tokens.surface);
        }
    }

    pub fn render_frame(&self, frame: &mut Frame<'_>, area: Rect, context: &RenderContext) {
        self.render(area, frame.buffer_mut(), context);
    }
}
