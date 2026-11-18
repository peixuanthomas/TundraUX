use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::artwork::{
    AppIcon, AppIconCatalog, ArtItem, ArtSet, ExplorerIcon, TextArt, load_app_icons, load_art_set,
    load_explorer_icons, load_text_art,
};
use crate::asset_error::AssetError;
use crate::asset_manifest::{DEFAULT_THEME_ID, REQUIRED_TEXT_ARTS};
use crate::asset_resolver::AssetResolver;
use crate::clock_font::{ClockFontAsset, load_clock_font};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AssetDimensions {
    pub width: usize,
    pub height: usize,
}

impl AssetDimensions {
    fn include(&mut self, width: usize, height: usize) {
        self.width = self.width.max(width);
        self.height = self.height.max(height);
    }
}

#[derive(Debug, Clone)]
pub struct AsciiAssetStore {
    resolver: AssetResolver,
    theme_id: String,
    banners: ArtSet,
    explorer_icons: ArtSet,
    app_icons: AppIconCatalog,
    image_assets: BTreeMap<String, Vec<u8>>,
    load_images: bool,
    clock_font: ClockFontAsset,
    text_arts: BTreeMap<String, TextArt>,
}

impl AsciiAssetStore {
    /// Repairs damaged default files and loads them, using embedded bytes in memory
    /// for files that cannot be restored. Strict loading APIs remain available.
    pub fn load_default_with_recovery()
    -> Result<(Self, crate::DefaultThemeRecoveryReport), AssetError> {
        let root = crate::asset_root_for_recovery_from_env_or_current_exe()?;
        Self::load_default_with_root_and_recovery(root)
    }

    /// Like `load_default_with_recovery`, with an explicit root that may be absent
    /// or unwritable. The returned store retains this root, including image paths.
    /// A fallback image path need not exist on disk; use the image-bytes APIs to
    /// access its content. `reload` retains the store's memory fallbacks.
    ///
    /// Default PNGs use the existing exact embedded-content integrity checks.
    /// Valid custom text/catalog files and separately referenced custom images are
    /// preserved on disk. If a custom image cannot be read, its owning catalog
    /// falls back to the complete embedded catalog in memory, including its default
    /// image references. Other healthy catalogs remain active.
    pub fn load_default_with_root_and_recovery(
        root: impl Into<PathBuf>,
    ) -> Result<(Self, crate::DefaultThemeRecoveryReport), AssetError> {
        crate::asset_restore::recover_default_theme(&root.into())
    }

    pub fn load_default() -> Result<Self, AssetError> {
        Self::load_theme(DEFAULT_THEME_ID)
    }

    /// Loads every catalog, font and text asset for a host that cannot display images.
    pub fn load_default_without_images() -> Result<Self, AssetError> {
        Self::load_with_resolver_and_images(
            AssetResolver::from_env_or_current_exe()?,
            DEFAULT_THEME_ID,
            false,
        )
    }

    pub fn load_theme(theme_id: &str) -> Result<Self, AssetError> {
        Self::load_with_resolver(AssetResolver::from_env_or_current_exe()?, theme_id)
    }

    pub fn load_with_root(root: impl Into<PathBuf>, theme_id: &str) -> Result<Self, AssetError> {
        Self::load_with_resolver(AssetResolver::new(root.into())?, theme_id)
    }

    pub fn load_with_resolver(resolver: AssetResolver, theme_id: &str) -> Result<Self, AssetError> {
        Self::load_with_resolver_and_images(resolver, theme_id, true)
    }

    fn load_with_resolver_and_images(
        resolver: AssetResolver,
        theme_id: &str,
        load_images: bool,
    ) -> Result<Self, AssetError> {
        let banners = load_art_set(&resolver, theme_id, "banner", "banner.toml")?;
        let explorer_icons = load_explorer_icons(&resolver, theme_id)?;
        let mut icon_resolver = resolver.clone();
        let app_icons = load_app_icons(&mut icon_resolver, theme_id)?;
        let image_assets = if load_images {
            load_image_assets(&icon_resolver, theme_id, &app_icons)?
        } else {
            BTreeMap::new()
        };
        let clock_font = load_clock_font(&resolver, theme_id)?;
        let mut text_arts = BTreeMap::new();
        for (key, relative_path) in REQUIRED_TEXT_ARTS {
            let art = load_text_art(&resolver, theme_id, key, relative_path)?;
            text_arts.insert((*key).to_string(), art);
        }

        Ok(Self {
            resolver,
            theme_id: theme_id.to_string(),
            banners,
            explorer_icons,
            app_icons,
            image_assets,
            load_images,
            clock_font,
            text_arts,
        })
    }

    pub fn reload(&mut self) -> Result<(), AssetError> {
        *self = Self::load_with_resolver_and_images(
            self.resolver.clone(),
            &self.theme_id,
            self.load_images,
        )?;
        Ok(())
    }

    pub fn root(&self) -> &Path {
        self.resolver.root()
    }

    pub fn theme_id(&self) -> &str {
        &self.theme_id
    }

    pub fn banner_lines(&self, key: &str) -> Result<&[String], AssetError> {
        self.banners
            .get(key)
            .map(ArtItem::lines)
            .ok_or_else(|| AssetError::UnknownAsset {
                asset: format!("banner/{key}"),
            })
    }

    pub fn app_icon_catalog(&self) -> &AppIconCatalog {
        &self.app_icons
    }

    pub fn app_icon(&self, key: &str) -> Option<&AppIcon> {
        self.app_icons.icon(key)
    }

    pub fn app_icon_image_path(&self, key: &str) -> Option<PathBuf> {
        let relative_path = self.app_icon(key)?.image_path()?;
        self.image_assets
            .contains_key(relative_path)
            .then(|| self.resolver.asset_path(&self.theme_id, relative_path))
    }

    pub fn app_icon_image_bytes(&self, key: &str) -> Option<&[u8]> {
        let relative_path = self.app_icon(key)?.image_path()?;
        self.image_assets.get(relative_path).map(Vec::as_slice)
    }

    pub fn explorer_icon(&self, key: &str) -> Result<&ExplorerIcon, AssetError> {
        self.explorer_icons
            .get(key)
            .ok_or_else(|| AssetError::UnknownAsset {
                asset: format!("explorer_icons/{key}"),
            })
    }

    pub fn explorer_icons(&self) -> impl Iterator<Item = &ExplorerIcon> {
        self.explorer_icons.items()
    }

    pub fn clock_font(&self) -> &ClockFontAsset {
        &self.clock_font
    }

    pub fn text_art(&self, key: &str) -> Result<&TextArt, AssetError> {
        self.text_arts
            .get(key)
            .ok_or_else(|| AssetError::UnknownAsset {
                asset: key.to_string(),
            })
    }

    pub fn max_asset_dimensions(&self) -> AssetDimensions {
        let mut dimensions = AssetDimensions::default();

        for item in self
            .banners
            .items()
            .chain(self.app_icons.icons())
            .chain(self.explorer_icons.items())
        {
            dimensions.include(item.width(), item.height());
        }
        for art in self.text_arts.values() {
            dimensions.include(art.width(), art.height());
        }
        dimensions.include(
            self.clock_font.max_rendered_clock_width(),
            self.clock_font.height,
        );

        dimensions
    }
}

fn load_image_assets(
    resolver: &AssetResolver,
    theme_id: &str,
    app_icons: &AppIconCatalog,
) -> Result<BTreeMap<String, Vec<u8>>, AssetError> {
    let relative_paths = app_icons
        .icons()
        .filter_map(ArtItem::image_path)
        .map(str::to_string)
        .collect::<BTreeSet<_>>();
    let mut images = BTreeMap::new();

    for relative_path in relative_paths {
        let bytes = resolver.read_asset(theme_id, &relative_path, &relative_path)?;
        images.insert(relative_path, bytes);
    }

    Ok(images)
}

#[cfg(test)]
#[path = "../tests/unit/asset_store.rs"]
mod tests;
