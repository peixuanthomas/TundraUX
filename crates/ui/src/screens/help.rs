use crate::{
    NotificationActionViewModel, NotificationLevel, NotificationTone, NotificationViewModel,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageHelpScreen {
    Home,
    Setup,
    Explorer,
    Launcher,
    CommandLine,
    Editor,
    Pdf,
    Script,
    Settings,
    Logs,
    Management,
    Packages,
    Services,
    Processes,
    ScheduledTasks,
    Diagnostics,
    SystemStatus,
    Clock,
}

impl PageHelpScreen {
    pub const ALL: [Self; 18] = [
        Self::Home,
        Self::Setup,
        Self::Explorer,
        Self::Launcher,
        Self::CommandLine,
        Self::Editor,
        Self::Pdf,
        Self::Script,
        Self::Settings,
        Self::Logs,
        Self::Management,
        Self::Packages,
        Self::Services,
        Self::Processes,
        Self::ScheduledTasks,
        Self::Diagnostics,
        Self::SystemStatus,
        Self::Clock,
    ];
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageHelpRow {
    pub keys: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageHelpSection {
    pub title: String,
    pub rows: Vec<PageHelpRow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageHelpViewModel {
    pub title: String,
    pub sections: Vec<PageHelpSection>,
    pub close_label: String,
}

fn row(keys: &str, description: String) -> PageHelpRow {
    PageHelpRow {
        keys: keys.into(),
        description,
    }
}

fn section(title: String, rows: Vec<PageHelpRow>) -> PageHelpSection {
    PageHelpSection { title, rows }
}

fn actions(rows: Vec<PageHelpRow>) -> PageHelpSection {
    section(i18n::tr!("ui-help-actions"), rows)
}

fn advanced(rows: Vec<PageHelpRow>) -> PageHelpSection {
    section(i18n::tr!("ui-help-advanced"), rows)
}

fn navigation() -> PageHelpSection {
    section(
        i18n::tr!("ui-help-navigation"),
        vec![
            row("Esc", i18n::tr!("ui-help-nav-back")),
            row("F1", i18n::tr!("ui-help-nav-help")),
            row("Tab", i18n::tr!("ui-help-nav-tabs")),
            row("Enter", i18n::tr!("ui-help-nav-activate")),
        ],
    )
}

fn workspace_navigation() -> PageHelpSection {
    section(
        i18n::tr!("ui-workspace-title"),
        vec![
            row("Ctrl+W, W", i18n::tr!("ui-help-nav-windows")),
            row("Ctrl+W, B", i18n::tr!("ui-workspace-background")),
            row(
                "Ctrl+W, h/j/k/l / ←→↑↓",
                i18n::tr!("ui-help-workspace-focus"),
            ),
            row("Ctrl+W, n", i18n::tr!("notification-panel-title")),
            row("Ctrl+W, v", i18n::tr!("ui-workspace-left-right")),
            row("Ctrl+W, s", i18n::tr!("ui-workspace-top-bottom")),
            row("Ctrl+W", i18n::tr!("ui-help-workspace-native")),
            row("Ctrl+W, o", i18n::tr!("ui-help-workspace-maximize")),
            row("Ctrl+Q / Ctrl+W, Q", i18n::tr!("ui-help-workspace-close")),
            row("Ctrl+W, Shift+X", i18n::tr!("ui-help-workspace-kill")),
            row("Ctrl+W, Ctrl+W", i18n::tr!("ui-help-workspace-send")),
        ],
    )
}

fn management_selection_help(mut rows: Vec<PageHelpRow>) -> Vec<PageHelpSection> {
    rows.insert(0, row("Shift+Click", i18n::tr!("ui-help-selection-range")));
    rows.insert(0, row("Shift+Enter", i18n::tr!("ui-help-selection-toggle")));
    rows.insert(
        0,
        row(
            if cfg!(target_os = "macos") {
                "Cmd+Click"
            } else {
                "Ctrl+Click"
            },
            i18n::tr!("ui-help-selection-toggle"),
        ),
    );
    vec![
        actions(rows),
        advanced(vec![
            row(
                "↑↓ / Home / End / PgUp / PgDn",
                i18n::tr!("ui-help-selection-move"),
            ),
            row("Tab / Shift+Tab", i18n::tr!("ui-help-management-focus")),
            row("Enter / Space", i18n::tr!("ui-help-nav-activate")),
            row("F4", i18n::tr!("ui-help-management-details")),
            row("F5", i18n::tr!("ui-help-explorer-refresh")),
            row("/", i18n::tr!("ui-help-explorer-search")),
            row("", i18n::tr!("ui-help-selection-lifecycle")),
        ]),
    ]
}

pub fn page_help(screen: PageHelpScreen) -> PageHelpViewModel {
    let (title, pages) = match screen {
        PageHelpScreen::Home => (
            i18n::tr!("ui-help-home-title"),
            vec![
                actions(vec![
                    row("Enter / Space", i18n::tr!("ui-help-home-entries")),
                    row("F2", i18n::tr!("ui-help-home-edit")),
                ]),
                advanced(vec![
                    row("←→↑↓", i18n::tr!("ui-help-home-move")),
                    row("Shift+←→", i18n::tr!("ui-help-home-reorder")),
                    row("Esc", i18n::tr!("ui-help-home-edit-cancel")),
                ]),
            ],
        ),
        PageHelpScreen::Explorer => (
            i18n::tr!("ui-help-explorer-title"),
            vec![
                actions(vec![
                    row("Enter", i18n::tr!("ui-help-explorer-open")),
                    row("N / T", i18n::tr!("ui-help-explorer-new")),
                    row("F2", i18n::tr!("ui-help-explorer-rename")),
                    row("Del", i18n::tr!("ui-help-explorer-delete")),
                    row("C / X / V", i18n::tr!("ui-help-explorer-clipboard")),
                    row(
                        crate::ExplorerToolbarAction::Undo.shortcut_label(),
                        i18n::tr!("ui-explorer-undo-move"),
                    ),
                    row("Shift+F10", i18n::tr!("ui-help-explorer-menu")),
                    row("F6", i18n::tr!("ui-help-explorer-sort")),
                    row("O", i18n::tr!("ui-help-explorer-options")),
                ]),
                advanced(vec![
                    row("Tab / Shift+Tab", i18n::tr!("ui-help-explorer-focus")),
                    row("↑↓", i18n::tr!("ui-help-explorer-rows")),
                    row("← / →", i18n::tr!("ui-help-explorer-history")),
                    row("Backspace", i18n::tr!("ui-explorer-up")),
                    row("Esc", i18n::tr!("ui-help-explorer-escape")),
                    row("Ctrl/Cmd+L", i18n::tr!("ui-explorer-edit-button")),
                    row("Ctrl/Cmd+A", i18n::tr!("ui-help-explorer-select-all")),
                    row("Ctrl/Cmd+I", i18n::tr!("ui-explorer-invert-selection")),
                    row("Ctrl/Cmd+Shift+A", i18n::tr!("ui-explorer-clear-selection")),
                    row("Shift+Enter", i18n::tr!("ui-help-selection-toggle")),
                    row("F5", i18n::tr!("ui-help-explorer-refresh")),
                    row("/ · Ctrl/Cmd+F", i18n::tr!("ui-help-explorer-search")),
                    row("F10", i18n::tr!("ui-help-explorer-save")),
                ]),
            ],
        ),
        PageHelpScreen::Launcher => (
            i18n::tr!("ui-help-launcher-title"),
            vec![
                actions(vec![
                    row("Ctrl/Cmd+F / Tab", i18n::tr!("ui-launcher-search-help")),
                    row("Enter", i18n::tr!("ui-help-launcher-open")),
                    row("V", i18n::tr!("ui-help-launcher-view")),
                    row("F4", i18n::tr!("ui-help-launcher-run-settings")),
                    row("F2", i18n::tr!("ui-launcher-organize")),
                    row("Ins", i18n::tr!("ui-launcher-add")),
                    row("Del", i18n::tr!("ui-help-launcher-remove")),
                    row("Shift+←/→", i18n::tr!("ui-launcher-reorder-help")),
                    row("PgUp/PgDn", i18n::tr!("ui-launcher-page-help")),
                ]),
                advanced(vec![
                    row("R/F5", i18n::tr!("ui-help-launcher-refresh")),
                    row("←→↑↓", i18n::tr!("ui-help-launcher-move")),
                ]),
            ],
        ),
        PageHelpScreen::Logs => (
            i18n::tr!("ui-help-logs-title"),
            vec![
                actions(vec![
                    row("Enter", i18n::tr!("ui-help-logs-open")),
                    row("P", i18n::tr!("ui-help-logs-follow")),
                    row("F", i18n::tr!("ui-help-logs-filter")),
                    row("L", i18n::tr!("ui-help-logs-level")),
                    row("G", i18n::tr!("ui-help-logs-folder")),
                    row("F10", i18n::tr!("ui-help-logs-more")),
                ]),
                advanced(vec![
                    row("R/F5", i18n::tr!("ui-help-logs-refresh")),
                    row("←/→", i18n::tr!("ui-help-logs-source")),
                    row("Esc", i18n::tr!("ui-help-logs-back")),
                ]),
            ],
        ),
        PageHelpScreen::Settings => (
            i18n::tr!("ui-help-settings-title"),
            vec![
                actions(vec![row("Enter / ␣", i18n::tr!("ui-help-settings-open"))]),
                advanced(vec![
                    row("←→↑↓", i18n::tr!("ui-help-settings-adjust")),
                    row("Tab / Shift+Tab", i18n::tr!("ui-help-settings-section")),
                    row("←→ / 0–9 / Enter", i18n::tr!("ui-help-settings-number")),
                    row("F5", i18n::tr!("ui-help-settings-update-refresh")),
                    row(
                        "↑↓ / PgUp PgDn / Enter",
                        i18n::tr!("ui-help-settings-update-tree"),
                    ),
                ]),
            ],
        ),
        PageHelpScreen::Packages => (
            i18n::tr!("ui-help-packages-title"),
            vec![
                actions(vec![
                    row("Shift+Enter", i18n::tr!("ui-help-selection-toggle")),
                    row(
                        if cfg!(target_os = "macos") {
                            "Cmd+Click"
                        } else {
                            "Ctrl+Click"
                        },
                        i18n::tr!("ui-help-selection-toggle"),
                    ),
                    row("Shift+Click", i18n::tr!("ui-help-selection-range")),
                    row("I / X / U", i18n::tr!("ui-help-packages-apply")),
                    row("Enter / F4", i18n::tr!("ui-help-management-details")),
                ]),
                advanced(vec![
                    row("↑↓", i18n::tr!("ui-help-selection-move")),
                    row("Tab / Shift+Tab", i18n::tr!("ui-help-management-focus")),
                    row("F7 / F8 / F6", i18n::tr!("ui-help-packages-pages")),
                    row("F5", i18n::tr!("ui-help-explorer-refresh")),
                    row("/", i18n::tr!("ui-help-explorer-search")),
                ]),
            ],
        ),
        PageHelpScreen::Services => (
            i18n::tr!("management-services"),
            management_selection_help(vec![
                row("A / X / T", i18n::tr!("ui-help-services-batch-state")),
                row("E / D", i18n::tr!("ui-help-services-batch-startup")),
                row("", i18n::tr!("ui-help-services-batch-more")),
            ]),
        ),
        PageHelpScreen::Processes => (
            i18n::tr!("management-processes"),
            management_selection_help(vec![
                row("P / C", i18n::tr!("ui-help-processes-batch-state")),
                row("T / K", i18n::tr!("ui-help-processes-batch-stop")),
                row("N", i18n::tr!("ui-help-processes-batch-priority")),
            ]),
        ),
        PageHelpScreen::ScheduledTasks => (
            i18n::tr!("management-scheduled-tasks"),
            management_selection_help(vec![
                row("", i18n::tr!("ui-help-scheduled-batch-actions")),
                row("", i18n::tr!("scheduled-task-cron-results-notice")),
                row("", i18n::tr!("scheduled-task-pause-notice")),
            ]),
        ),
        PageHelpScreen::Management => (
            i18n::tr!("ui-help-management-title"),
            vec![
                actions(vec![
                    row("Enter", i18n::tr!("ui-help-management-action")),
                    row("Tab", i18n::tr!("ui-help-management-focus")),
                ]),
                advanced(vec![
                    row("F4", i18n::tr!("ui-help-management-details")),
                    row("Alt+Left Alt+Right", i18n::tr!("ui-help-management-pages")),
                ]),
            ],
        ),
        PageHelpScreen::SystemStatus => (
            i18n::tr!("ui-help-system-status-title"),
            vec![
                actions(vec![
                    row("E", i18n::tr!("ui-help-system-status-edit")),
                    row("R/F5", i18n::tr!("ui-help-system-status-refresh")),
                    row("H", i18n::tr!("ui-help-system-status-diagnostics")),
                    row("Ctrl+S", i18n::tr!("ui-help-system-status-save")),
                    row("A", i18n::tr!("ui-help-system-status-add")),
                    row("F4", i18n::tr!("ui-help-system-status-size")),
                    row("Del", i18n::tr!("ui-help-system-status-remove")),
                    row("Esc", i18n::tr!("ui-help-system-status-cancel")),
                ]),
                advanced(vec![row(
                    "Shift+←→↑↓ / Enter",
                    i18n::tr!("ui-help-system-status-move"),
                )]),
            ],
        ),
        PageHelpScreen::Diagnostics => (
            i18n::tr!("ui-help-diagnostics-title"),
            vec![
                actions(vec![
                    row("R", i18n::tr!("ui-help-diagnostics-rescan")),
                    row("F", i18n::tr!("ui-help-diagnostics-repair")),
                    row("A", i18n::tr!("ui-help-diagnostics-repair-all")),
                ]),
                advanced(vec![
                    row("C", i18n::tr!("ui-help-diagnostics-copy")),
                    row("O", i18n::tr!("ui-help-diagnostics-open")),
                    row("X", i18n::tr!("ui-help-diagnostics-restart")),
                ]),
            ],
        ),
        PageHelpScreen::Clock => (
            i18n::tr!("ui-help-clock-title"),
            vec![
                actions(vec![
                    row("N", i18n::tr!("ui-help-clock-new")),
                    row("M", i18n::tr!("ui-help-clock-manage")),
                ]),
                advanced(vec![
                    row("F3 F4", i18n::tr!("ui-help-clock-create")),
                    row("←→ / Tab / ↑↓ / 0–9", i18n::tr!("ui-help-clock-input")),
                    row("Enter Esc", i18n::tr!("ui-help-clock-confirm")),
                ]),
            ],
        ),
        PageHelpScreen::Setup => (
            i18n::tr!("ui-help-setup-title"),
            vec![
                actions(vec![
                    row("Enter", i18n::tr!("ui-help-auth-submit")),
                    row("Esc", i18n::tr!("ui-help-auth-back")),
                ]),
                advanced(vec![row("Tab / ←→↑↓", i18n::tr!("ui-help-auth-focus"))]),
            ],
        ),
        PageHelpScreen::Script => (
            i18n::tr!("app-catalog-script-editor-name"),
            vec![
                actions(vec![
                    row("Ctrl+F5", i18n::tr!("script-help-run")),
                    row("F5", i18n::tr!("script-help-debug")),
                    row("F7 / F11 / Shift+F11", i18n::tr!("script-help-step")),
                    row("Shift+F5", i18n::tr!("script-help-stop")),
                    row("F9", i18n::tr!("script-help-breakpoint")),
                    row("B / ? / ! / . / >", i18n::tr!("script-help-markers")),
                ]),
                advanced(vec![
                    row("F6 / Shift+F6", i18n::tr!("script-help-panes")),
                    row("Tab / Shift+Tab", i18n::tr!("script-help-controls")),
                    row("Enter / Ctrl+Enter", i18n::tr!("script-help-expression")),
                    row("Ctrl+C / Ctrl+V", i18n::tr!("ui-help-terminal-clipboard")),
                    row("Ctrl+\\", i18n::tr!("ui-help-terminal-interrupt")),
                    row("Shift+F10", i18n::tr!("ui-help-terminal-menu")),
                    row("F8", i18n::tr!("script-help-vim")),
                    row("F10", i18n::tr!("script-help-menu")),
                ]),
            ],
        ),
        PageHelpScreen::Editor => (
            i18n::tr!("ui-help-editor-title"),
            vec![
                actions(vec![
                    row("Ctrl+O", i18n::tr!("ui-help-editor-open-preview")),
                    row("Ctrl+S", i18n::tr!("ui-help-editor-save")),
                    row("Ctrl+F", i18n::tr!("ui-help-editor-find")),
                    row("R", i18n::tr!("ui-help-editor-reload")),
                    row("PgUp/PgDn", i18n::tr!("ui-help-editor-scroll-picture")),
                ]),
                advanced(vec![
                    row("Shift+F10", i18n::tr!("ui-editor-edit")),
                    row("F8", i18n::tr!("ui-help-editor-vim")),
                    row("F6", i18n::tr!("ui-help-editor-focus")),
                    row("Tab / ←→ / Enter", i18n::tr!("ui-help-editor-settings")),
                ]),
            ],
        ),
        PageHelpScreen::Pdf => (
            i18n::tr!("ui-help-pdf-title"),
            vec![actions(vec![
                row("PgUp", i18n::tr!("ui-editor-pdf-previous")),
                row("PgDn", i18n::tr!("ui-help-pdf-next")),
                row("Ctrl+G", i18n::tr!("ui-editor-pdf-jump")),
                row("R", i18n::tr!("ui-help-pdf-rotate")),
                row("+", i18n::tr!("ui-editor-preview-zoom-in")),
                row("-", i18n::tr!("ui-editor-preview-zoom-out")),
                row("← ↑ ↓ →", i18n::tr!("ui-help-editor-pan-preview")),
                row("Ctrl+O", i18n::tr!("ui-editor-pdf-open")),
            ])],
        ),
        PageHelpScreen::CommandLine => (
            i18n::tr!("ui-help-command-line-title"),
            vec![
                actions(vec![row(
                    &i18n::tr!("ui-help-back-button"),
                    i18n::tr!("ui-help-command-line-close"),
                )]),
                advanced(vec![
                    row("Ctrl+C / Ctrl+V", i18n::tr!("ui-help-terminal-clipboard")),
                    row("Ctrl+\\", i18n::tr!("ui-help-terminal-interrupt")),
                    row("Shift+F10", i18n::tr!("ui-help-terminal-menu")),
                ]),
            ],
        ),
    };
    let mut sections = pages;
    sections.push(navigation());
    if screen != PageHelpScreen::Setup {
        sections.push(workspace_navigation());
    }
    PageHelpViewModel {
        title,
        sections,
        close_label: i18n::tr!("ui-help-close"),
    }
}

pub fn page_help_notification(
    help: &PageHelpViewModel,
    scroll_offset: usize,
) -> NotificationViewModel {
    let mut message = String::new();
    for section in &help.sections {
        if !message.is_empty() {
            message.push_str("\n\n");
        }
        message.push_str(&section.title);
        for row in &section.rows {
            message.push('\n');
            message.push_str(&row.keys);
            message.push_str("  ");
            message.push_str(&row.description);
        }
    }
    let mut model = NotificationViewModel::new(
        "page-help",
        NotificationLevel::Modal,
        NotificationTone::Info,
        help.title.clone(),
        message,
        vec![NotificationActionViewModel::new("close", help.close_label.clone()).cancel(true)],
    );
    model.stacked_actions = true;
    model.actions[0].selected = true;
    model.scroll_offset = scroll_offset;
    model.align_left = true;
    model.show_tone_prefix = false;
    model
}

pub fn render_page_help_overlay(
    frame: &mut ratatui::Frame<'_>,
    area: ratatui::layout::Rect,
    help: &PageHelpViewModel,
    scroll: usize,
    context: &crate::RenderContext,
) {
    let model = page_help_notification(help, scroll);
    crate::screens::notifications::render_notification_overlay_context(
        frame, area, &model, context,
    );
}

#[cfg(test)]
#[path = "../../tests/unit/screens/help/tests.rs"]
mod tests;
