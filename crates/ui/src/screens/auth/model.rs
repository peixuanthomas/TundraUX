pub use app::{SetupLanguageOption, SetupTimezoneOption};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupStep {
    Language,
    Timezone,
    Appearance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupField {
    LanguageList,
    TimezoneList,
    AppearanceShape,
    AppearanceThemeColor,
    AppearanceThemeCustom,
    AppearanceAccentColor,
    AppearanceAccentCustom,
    AppearanceSubmit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupCustomColorTarget {
    Theme,
    Accent,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SetupViewModel {
    pub scroll_offset: u16,
    pub step: SetupStep,
    pub languages: Vec<SetupLanguageOption>,
    pub timezones: Vec<SetupTimezoneOption>,
    pub selected_language_index: usize,
    pub selected_timezone_index: usize,
    pub timezone_window_start: usize,
    pub focused_field: SetupField,
    pub defaults_available: bool,
    pub border_shape: crate::BorderShape,
    pub theme_color: ratatui::style::Color,
    pub theme_color_value: String,
    pub accent_color: ratatui::style::Color,
    pub accent_color_value: String,
    pub custom_color_target: Option<SetupCustomColorTarget>,
    pub custom_color_input: String,
    pub custom_color_editing: crate::components::TextEditState,
    pub custom_color_valid: bool,
    pub custom_color_conflicts_with_theme: bool,
    pub custom_color_error: Option<String>,
    pub error: Option<String>,
}

impl SetupViewModel {
    pub fn selected_language(&self) -> Option<&SetupLanguageOption> {
        self.languages.get(self.selected_language_index)
    }

    pub fn selected_timezone(&self) -> Option<&SetupTimezoneOption> {
        self.timezones.get(self.selected_timezone_index)
    }
}
