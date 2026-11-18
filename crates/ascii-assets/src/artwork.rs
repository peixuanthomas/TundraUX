use std::borrow::Cow;
use std::collections::{BTreeMap, btree_map::Entry};
use std::path::{Component, Path};

use serde::Deserialize;

use crate::asset_error::AssetError;
use crate::asset_resolver::AssetResolver;
use crate::embedded_defaults::embedded_default_theme_file;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextArt {
    key: String,
    lines: Vec<String>,
    width: usize,
    height: usize,
}

impl TextArt {
    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    pub fn to_vec(&self) -> Vec<String> {
        self.lines.clone()
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtItem {
    pub key: String,
    pub label: Option<String>,
    image_path: Option<String>,
    pub lines: Vec<String>,
    pub width: usize,
    pub height: usize,
}

impl ArtItem {
    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn label(&self) -> Option<&str> {
        self.label.as_deref()
    }

    pub fn lines(&self) -> &[String] {
        &self.lines
    }

    pub fn image_path(&self) -> Option<&str> {
        self.image_path.as_deref()
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }
}

pub type AppIcon = ArtItem;
pub type ExplorerIcon = ArtItem;

pub(crate) const EXPLORER_ENTRY_AND_LOCATION_ICON_KEYS: &[&str] = &[
    "folder",
    "file",
    "text",
    "code",
    "document",
    "image",
    "audio",
    "video",
    "archive",
    "executable",
    "link",
    "other",
    "desktop",
    "documents",
    "downloads",
    "pictures",
    "music",
    "videos",
    "drive",
    "trash",
];

pub(crate) const EXPLORER_ACTION_ICON_KEYS: &[&str] = &[
    "back",
    "forward",
    "up",
    "refresh",
    "new",
    "cut",
    "copy",
    "paste",
    "rename",
    "delete",
    "search",
    "options",
    "sort_asc",
    "sort_desc",
    "cancel",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArtSet {
    items: BTreeMap<String, ArtItem>,
}

impl ArtSet {
    pub fn get(&self, key: &str) -> Option<&ArtItem> {
        self.items.get(key)
    }

    pub fn items(&self) -> impl Iterator<Item = &ArtItem> {
        self.items.values()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppIconCatalog {
    icons: BTreeMap<String, AppIcon>,
}

impl AppIconCatalog {
    pub fn icon(&self, key: &str) -> Option<&AppIcon> {
        let id = app_icon_id(key);
        self.icons.get(id.as_ref()).or_else(|| {
            if id == "builtin.script-editor" {
                self.icons.get("builtin.editor")
            } else {
                None
            }
        })
    }

    pub fn icon_for_label(&self, label: &str) -> Option<&AppIcon> {
        self.icon(label)
            .or_else(|| self.icons().find(|icon| icon.label() == Some(label)))
            .or_else(|| self.icon("default"))
    }

    pub fn icons(&self) -> impl Iterator<Item = &AppIcon> {
        self.icons.values()
    }
}

fn app_icon_id(key: &str) -> Cow<'_, str> {
    match key {
        "system_status" => Cow::Borrowed("builtin.system-status"),
        "user_management" => Cow::Borrowed("builtin.users"),
        "default" => Cow::Borrowed(key),
        key if key.starts_with("builtin.") => Cow::Borrowed(key),
        key => Cow::Owned(format!("builtin.{key}")),
    }
}

pub(crate) fn load_text_art(
    resolver: &AssetResolver,
    theme_id: &str,
    key: &str,
    relative_path: &str,
) -> Result<TextArt, AssetError> {
    let source = read_asset_to_string(resolver, theme_id, key, relative_path)?;
    let lines = split_preserved_lines(&source);
    ensure_ascii_lines(key, "text art", &lines)?;
    let (width, height) = measure_lines(&lines);
    if height == 0 {
        return Err(AssetError::InvalidAsset {
            asset: key.to_string(),
            message: "text art must contain at least one line".to_string(),
        });
    }

    Ok(TextArt {
        key: key.to_string(),
        lines,
        width,
        height,
    })
}

pub(crate) fn load_app_icons(
    resolver: &mut AssetResolver,
    theme_id: &str,
) -> Result<AppIconCatalog, AssetError> {
    let icons = match load_art_set(resolver, theme_id, "app_icons", "app_icons.toml") {
        Ok(current) => current.items,
        Err(missing @ AssetError::MissingAsset { .. }) => {
            let home = match load_art_set(resolver, theme_id, "home_icons", "home_icons.toml") {
                Err(AssetError::MissingAsset { .. }) => return Err(missing),
                result => result?,
            };
            let launcher =
                load_art_set(resolver, theme_id, "launcher_icons", "launcher_icons.toml")?;
            let mut icons: BTreeMap<_, _> = home
                .items
                .into_values()
                .map(|mut icon| {
                    icon.key = app_icon_id(&icon.key).into_owned();
                    (icon.key.clone(), icon)
                })
                .collect();
            // Older themes can define an explicit application ID in the Launcher catalog.
            icons.extend(launcher.items);
            if theme_id == crate::DEFAULT_THEME_ID {
                let catalog = embedded_default_theme_file("app_icons").expect("embedded app icons");
                let mut embedded = resolver.clone();
                embedded.use_embedded_default(catalog.relative_path, catalog.contents);
                let defaults =
                    load_art_set(&embedded, theme_id, catalog.key, catalog.relative_path)?;
                for (key, icon) in defaults.items {
                    if let Entry::Vacant(entry) = icons.entry(key) {
                        if let Some(path) = icon.image_path() {
                            let file =
                                embedded_default_theme_file(path).expect("embedded app icon");
                            resolver.use_embedded_default(file.relative_path, file.contents);
                        }
                        entry.insert(icon);
                    }
                }
            }
            icons
        }
        Err(error) => return Err(error),
    };

    let core = [
        "default",
        "builtin.explorer",
        "builtin.launcher",
        "builtin.settings",
        "builtin.system-status",
        "builtin.users",
        "builtin.command-line",
        "builtin.editor",
    ];
    let default_apps: &[&str] = if theme_id == crate::DEFAULT_THEME_ID {
        &[
            "builtin.clock",
            "builtin.logs",
            "builtin.services",
            "builtin.processes",
            "builtin.packages",
            "builtin.network",
            "builtin.disks",
            "builtin.firewall",
            "builtin.ssh",
            "builtin.backups",
            "builtin.scheduled-tasks",
            "builtin.tasks",
        ]
    } else {
        &[]
    };
    for required in core.into_iter().chain(default_apps.iter().copied()) {
        if !icons.contains_key(required) {
            return Err(AssetError::InvalidAsset {
                asset: "app_icons".to_string(),
                message: format!("missing required built-in application icon {required}"),
            });
        }
    }
    if let Some(logs) = icons.get("builtin.logs")
        && (logs.width() != 7 || logs.height() != 4)
    {
        return Err(AssetError::InvalidAsset {
            asset: "app_icons".to_string(),
            message: "application icon builtin.logs must be exactly 7x4".to_string(),
        });
    }
    Ok(AppIconCatalog { icons })
}

pub(crate) fn load_explorer_icons(
    resolver: &AssetResolver,
    theme_id: &str,
) -> Result<ArtSet, AssetError> {
    let icons = load_art_set(resolver, theme_id, "explorer_icons", "explorer_icons.toml")?;
    for key in EXPLORER_ENTRY_AND_LOCATION_ICON_KEYS {
        let icon = icons.get(key).ok_or_else(|| AssetError::InvalidAsset {
            asset: "explorer_icons".to_string(),
            message: format!("missing required Explorer icon {key}"),
        })?;
        if icon.width() != 3 || icon.height() != 1 {
            return Err(AssetError::InvalidAsset {
                asset: "explorer_icons".to_string(),
                message: format!("Explorer entry/location icon {key} must be exactly 3x1"),
            });
        }
    }
    for key in EXPLORER_ACTION_ICON_KEYS {
        let icon = icons.get(key).ok_or_else(|| AssetError::InvalidAsset {
            asset: "explorer_icons".to_string(),
            message: format!("missing required Explorer action icon {key}"),
        })?;
        if icon.width() != 1 || icon.height() != 1 {
            return Err(AssetError::InvalidAsset {
                asset: "explorer_icons".to_string(),
                message: format!("Explorer action icon {key} must be exactly 1x1"),
            });
        }
    }

    Ok(icons)
}

pub(crate) fn load_art_set(
    resolver: &AssetResolver,
    theme_id: &str,
    key: &str,
    relative_path: &str,
) -> Result<ArtSet, AssetError> {
    let path = resolver.asset_path(theme_id, relative_path);
    let source = read_asset_to_string(resolver, theme_id, key, relative_path)?;
    let file: ArtSetFile = toml::from_str(&source).map_err(|source| AssetError::ParseToml {
        asset: key.to_string(),
        path,
        source: Box::new(source),
    })?;
    if file.schema_version != 1 {
        return Err(AssetError::InvalidAsset {
            asset: key.to_string(),
            message: format!("unsupported schema_version {}", file.schema_version),
        });
    }

    let mut items = BTreeMap::new();
    for (item_key, item_file) in file.items {
        let ArtItemFile {
            label,
            image,
            lines,
            body,
        } = item_file;
        let lines = match (lines, body) {
            (Some(lines), None) => lines,
            (None, Some(body)) => split_preserved_lines(&body),
            (Some(_), Some(_)) => {
                return Err(AssetError::InvalidAsset {
                    asset: key.to_string(),
                    message: format!("art item {item_key} must use either lines or body, not both"),
                });
            }
            (None, None) => {
                return Err(AssetError::InvalidAsset {
                    asset: key.to_string(),
                    message: format!("art item {item_key} is missing lines or body"),
                });
            }
        };
        ensure_ascii_lines(key, &format!("art item {item_key}"), &lines)?;
        let (width, height) = measure_lines(&lines);
        if height == 0 {
            return Err(AssetError::InvalidAsset {
                asset: key.to_string(),
                message: format!("art item {item_key} must contain at least one line"),
            });
        }
        let image_path = image
            .map(|image| validate_image_path(key, &item_key, image))
            .transpose()?;
        items.insert(
            item_key.clone(),
            ArtItem {
                key: item_key,
                label,
                image_path,
                lines,
                width,
                height,
            },
        );
    }

    Ok(ArtSet { items })
}

fn validate_image_path(
    asset_key: &str,
    item_key: &str,
    image: String,
) -> Result<String, AssetError> {
    let relative = Path::new(&image);
    if image.trim().is_empty()
        || relative.is_absolute()
        || !relative
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err(AssetError::InvalidAsset {
            asset: asset_key.to_string(),
            message: format!("art item {item_key} has an unsafe image path {image:?}"),
        });
    }
    let supported = relative
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "gif" | "jpeg" | "jpg" | "png" | "webp"
            )
        });
    if !supported {
        return Err(AssetError::InvalidAsset {
            asset: asset_key.to_string(),
            message: format!("art item {item_key} image must be GIF, JPEG, PNG, or WebP"),
        });
    }
    Ok(image)
}

pub(crate) fn read_asset_to_string(
    resolver: &AssetResolver,
    theme_id: &str,
    key: &str,
    relative_path: &str,
) -> Result<String, AssetError> {
    let path = resolver.asset_path(theme_id, relative_path);
    let bytes = resolver.read_asset(theme_id, key, relative_path)?;
    String::from_utf8(bytes).map_err(|source| AssetError::ReadAsset {
        asset: key.to_string(),
        path,
        source: std::io::Error::new(std::io::ErrorKind::InvalidData, source),
    })
}

fn split_preserved_lines(source: &str) -> Vec<String> {
    source
        .trim_end_matches(['\r', '\n'])
        .lines()
        .map(|line| line.trim_end_matches('\r').to_string())
        .collect()
}

fn measure_lines(lines: &[String]) -> (usize, usize) {
    (
        lines
            .iter()
            .map(|line| line.chars().count())
            .max()
            .unwrap_or(0),
        lines.len(),
    )
}

fn ensure_ascii_lines(asset: &str, description: &str, lines: &[String]) -> Result<(), AssetError> {
    if !lines_are_printable_ascii(lines) {
        return Err(AssetError::InvalidAsset {
            asset: asset.to_string(),
            message: format!("{description} must contain printable ASCII characters only"),
        });
    }

    Ok(())
}

pub(crate) fn lines_are_printable_ascii(lines: &[String]) -> bool {
    lines.iter().all(|line| {
        line.bytes()
            .all(|byte| byte == b' ' || byte.is_ascii_graphic())
    })
}

pub(crate) fn pad_lines(mut lines: Vec<String>) -> Vec<String> {
    let (width, _) = measure_lines(&lines);
    for line in &mut lines {
        let padding = width.saturating_sub(line.chars().count());
        line.push_str(&" ".repeat(padding));
    }
    lines
}

#[cfg(test)]
#[path = "../tests/unit/artwork/tests.rs"]
mod tests;

#[derive(Debug, Deserialize)]
struct ArtSetFile {
    schema_version: u16,
    #[allow(dead_code)]
    name: Option<String>,
    items: BTreeMap<String, ArtItemFile>,
}

#[derive(Debug, Deserialize)]
struct ArtItemFile {
    label: Option<String>,
    image: Option<String>,
    lines: Option<Vec<String>>,
    body: Option<String>,
}
