//! A command result marker composed as a styled span in terminal output.
use ratatui::style::Style;
use ratatui::text::Span;

use crate::TundraTheme;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandStatus {
    Pending,
    Succeeded,
    Failed,
}

impl CommandStatus {
    pub fn span(self, theme: &TundraTheme) -> Span<'static> {
        let (role, style) = match self {
            Self::Pending => ("pending", theme.muted_style()),
            Self::Succeeded => (
                "success",
                Style::default()
                    .fg(theme.tokens().success)
                    .bg(theme.background),
            ),
            Self::Failed => ("error", theme.error_style()),
        };
        Span::styled(crate::cell_icon("state", role), style)
    }
}
