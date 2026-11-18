use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub use ascii_assets::{
    AppIcon, AppIconCatalog, AsciiAssetStore, AssetDimensions, AssetError, ClockFontAsset,
    DEFAULT_THEME_ID, DefaultThemeCheckReport, DefaultThemeFile, ExplorerIcon,
    asset_root_for_recovery_from_env_or_current_exe, check_default_theme, default_theme_files,
    restore_default_theme,
};

#[derive(Clone)]
pub struct RuntimeAsciiAssets {
    store: Arc<AsciiAssetStore>,
}

impl RuntimeAsciiAssets {
    pub fn load_default() -> Result<Self, AssetError> {
        Ok(Self::from_store(AsciiAssetStore::load_default()?))
    }

    pub fn load_default_without_images() -> Result<Self, AssetError> {
        Ok(Self::from_store(
            AsciiAssetStore::load_default_without_images()?,
        ))
    }

    pub fn load_theme(theme_id: &str) -> Result<Self, AssetError> {
        Ok(Self::from_store(AsciiAssetStore::load_theme(theme_id)?))
    }

    pub fn load_with_root(root: &Path, theme_id: &str) -> Result<Self, AssetError> {
        Ok(Self::from_store(AsciiAssetStore::load_with_root(
            root, theme_id,
        )?))
    }

    pub fn from_store(store: AsciiAssetStore) -> Self {
        Self {
            store: Arc::new(store),
        }
    }

    pub fn from_shared_store(store: Arc<AsciiAssetStore>) -> Self {
        Self { store }
    }

    pub fn store(&self) -> &AsciiAssetStore {
        &self.store
    }

    pub fn shared_store(&self) -> Arc<AsciiAssetStore> {
        Arc::clone(&self.store)
    }

    pub fn theme_id(&self) -> &str {
        self.store.theme_id()
    }

    pub fn banner_lines(&self, key: &str) -> Result<&[String], AssetError> {
        self.store.banner_lines(key)
    }

    pub fn app_icon_catalog(&self) -> &AppIconCatalog {
        self.store.app_icon_catalog()
    }

    pub fn app_icon_image_path(&self, key: &str) -> Option<PathBuf> {
        self.store.app_icon_image_path(key)
    }

    pub fn app_icon_image_bytes(&self, key: &str) -> Option<&[u8]> {
        self.store.app_icon_image_bytes(key)
    }

    pub fn explorer_icon(&self, key: &str) -> Result<&ExplorerIcon, AssetError> {
        self.store.explorer_icon(key)
    }

    pub fn app_icon(&self, key: &str) -> Option<&AppIcon> {
        self.store.app_icon(key)
    }

    pub fn clock_font(&self) -> &ClockFontAsset {
        self.store.clock_font()
    }

    pub fn max_asset_dimensions(&self) -> AssetDimensions {
        self.store.max_asset_dimensions()
    }

    pub fn app_icon_for_label(&self, label: &str) -> Option<&AppIcon> {
        self.app_icon_catalog().icon_for_label(label)
    }
}

impl fmt::Debug for RuntimeAsciiAssets {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RuntimeAsciiAssets")
            .finish_non_exhaustive()
    }
}

impl PartialEq for RuntimeAsciiAssets {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.store, &other.store)
    }
}

impl Eq for RuntimeAsciiAssets {}

pub fn try_app_icon_for_label(label: &str) -> Result<AppIcon, AssetError> {
    let assets = RuntimeAsciiAssets::load_default()?;
    Ok(assets
        .app_icon_for_label(label)
        .cloned()
        .expect("app icon assets must define a default icon"))
}

pub fn app_icon_for_label(label: &str) -> AppIcon {
    try_app_icon_for_label(label).expect("default ASCII app icon assets must load")
}
