use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};
use unicode_width::UnicodeWidthStr;

use crate::ThemeTokens;

/// Paints progress with cell backgrounds for a uniform height around the label.
pub struct ProgressGauge<'a> {
    label: Span<'a>,
    ratio: f64,
    fill: Color,
    track: Color,
    theme: &'a ThemeTokens,
}

impl<'a> ProgressGauge<'a> {
    pub fn new(
        label: impl Into<Span<'a>>,
        ratio: f64,
        fill: Color,
        track: Color,
        theme: &'a ThemeTokens,
    ) -> Self {
        Self {
            label: label.into(),
            ratio: ratio.clamp(0.0, 1.0),
            fill,
            track,
            theme,
        }
    }
}

impl Widget for ProgressGauge<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        if area.is_empty() {
            return;
        }
        crate::automation::control(
            "",
            "progressbar",
            &self.label.content,
            area,
            Default::default(),
            &[],
        );
        let end = area.x + (f64::from(area.width) * self.ratio).round() as u16;
        for position in area.intersection(buffer.area).positions() {
            let background = if position.x < end {
                self.fill
            } else {
                self.track
            };
            buffer[position].set_symbol(" ").set_bg(background);
        }

        let width = usize::from(area.width).min(self.label.width()) as u16;
        let label_area = Rect::new(
            area.x + (area.width - width) / 2,
            area.y + area.height / 2,
            width,
            1,
        );
        let mut column = label_area.x;
        let spans = self
            .label
            .styled_graphemes(Style::default())
            .map(|grapheme| {
                // Style an entire grapheme from its starting cell, including a
                // wide glyph spanning the fill boundary, to avoid split text.
                let background = if column < end { self.fill } else { self.track };
                column = column.saturating_add(grapheme.symbol.width() as u16);
                Span::styled(
                    grapheme.symbol,
                    grapheme.style.patch(self.theme.filled_style(background)),
                )
            })
            .collect::<Vec<_>>();
        Paragraph::new(Line::from(spans)).render(label_area, buffer);
    }
}
