#[derive(Debug)]
pub(crate) struct EmbeddedDefaultThemeFile {
    pub key: &'static str,
    pub relative_path: &'static str,
    pub contents: &'static [u8],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefaultThemeFile {
    pub key: &'static str,
    pub relative_path: &'static str,
}

macro_rules! embedded_default_theme_file {
    ($key:literal, $relative_path:literal) => {
        EmbeddedDefaultThemeFile {
            key: $key,
            relative_path: $relative_path,
            contents: include_bytes!(concat!("../assets/themes/default/", $relative_path)),
        }
    };
}

// Keep one allocation shared by validation, recovery, and lookup. A `const`
// can materialize the entire table (including PNG bytes) at each use site.
pub(crate) static EMBEDDED_DEFAULT_THEME_FILES: &[EmbeddedDefaultThemeFile] = &[
    embedded_default_theme_file!("banner", "banner.toml"),
    embedded_default_theme_file!("explorer_icons", "explorer_icons.toml"),
    embedded_default_theme_file!("app_icons", "app_icons.toml"),
    embedded_default_theme_file!("app_icons/clock.png", "app_icons/clock.png"),
    embedded_default_theme_file!("app_icons/command_line.png", "app_icons/command_line.png"),
    embedded_default_theme_file!("app_icons/default.png", "app_icons/default.png"),
    embedded_default_theme_file!("app_icons/disks.png", "app_icons/disks.png"),
    embedded_default_theme_file!("app_icons/editor.png", "app_icons/editor.png"),
    embedded_default_theme_file!("app_icons/explorer.png", "app_icons/explorer.png"),
    embedded_default_theme_file!("app_icons/launcher.png", "app_icons/launcher.png"),
    embedded_default_theme_file!("app_icons/logs.png", "app_icons/logs.png"),
    embedded_default_theme_file!("app_icons/network.png", "app_icons/network.png"),
    embedded_default_theme_file!("app_icons/packages.png", "app_icons/packages.png"),
    embedded_default_theme_file!("app_icons/processes.png", "app_icons/processes.png"),
    embedded_default_theme_file!("app_icons/services.png", "app_icons/services.png"),
    embedded_default_theme_file!("app_icons/settings.png", "app_icons/settings.png"),
    embedded_default_theme_file!("app_icons/system_status.png", "app_icons/system_status.png"),
    embedded_default_theme_file!("app_icons/tasks.png", "app_icons/tasks.png"),
    embedded_default_theme_file!("app_icons/users.png", "app_icons/users.png"),
    embedded_default_theme_file!("weathr/render/clock_font", "weathr/render/clock_font.toml"),
    embedded_default_theme_file!("weathr/animation/airplane", "weathr/animation/airplane.txt"),
    embedded_default_theme_file!("weathr/animation/cloud_0", "weathr/animation/cloud_0.txt"),
    embedded_default_theme_file!("weathr/animation/cloud_1", "weathr/animation/cloud_1.txt"),
    embedded_default_theme_file!("weathr/animation/cloud_2", "weathr/animation/cloud_2.txt"),
    embedded_default_theme_file!("weathr/animation/cloud_3", "weathr/animation/cloud_3.txt"),
    embedded_default_theme_file!("weathr/animation/sun_0", "weathr/animation/sun_0.txt"),
    embedded_default_theme_file!("weathr/animation/sun_1", "weathr/animation/sun_1.txt"),
    embedded_default_theme_file!(
        "weathr/animation/moon/phase_0",
        "weathr/animation/moon/phase_0.txt"
    ),
    embedded_default_theme_file!(
        "weathr/animation/moon/phase_1",
        "weathr/animation/moon/phase_1.txt"
    ),
    embedded_default_theme_file!(
        "weathr/animation/moon/phase_2",
        "weathr/animation/moon/phase_2.txt"
    ),
    embedded_default_theme_file!(
        "weathr/animation/moon/phase_3",
        "weathr/animation/moon/phase_3.txt"
    ),
    embedded_default_theme_file!(
        "weathr/animation/moon/phase_4",
        "weathr/animation/moon/phase_4.txt"
    ),
    embedded_default_theme_file!(
        "weathr/animation/moon/phase_5",
        "weathr/animation/moon/phase_5.txt"
    ),
    embedded_default_theme_file!(
        "weathr/animation/moon/phase_6",
        "weathr/animation/moon/phase_6.txt"
    ),
    embedded_default_theme_file!(
        "weathr/animation/moon/phase_7",
        "weathr/animation/moon/phase_7.txt"
    ),
    embedded_default_theme_file!("weathr/world/fence", "weathr/world/fence.txt"),
    embedded_default_theme_file!("weathr/world/house", "weathr/world/house.txt"),
    embedded_default_theme_file!("weathr/world/mailbox", "weathr/world/mailbox.txt"),
    embedded_default_theme_file!("weathr/world/pine_tree", "weathr/world/pine_tree.txt"),
    embedded_default_theme_file!("weathr/world/tree", "weathr/world/tree.txt"),
];

pub(crate) fn embedded_default_theme_file(key: &str) -> Option<&'static EmbeddedDefaultThemeFile> {
    EMBEDDED_DEFAULT_THEME_FILES
        .iter()
        .find(|asset| asset.key == key)
}

pub fn default_theme_files() -> Vec<DefaultThemeFile> {
    EMBEDDED_DEFAULT_THEME_FILES
        .iter()
        .map(|file| DefaultThemeFile {
            key: file.key,
            relative_path: file.relative_path,
        })
        .collect()
}

#[cfg(test)]
#[path = "../tests/unit/embedded_defaults/tests.rs"]
mod tests;
