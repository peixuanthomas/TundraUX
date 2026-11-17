use std::path::PathBuf;

use platform::{AppPaths, Platform};

use crate::clock_document::ClockDocument;
use crate::config_document::StorageConfig;
use crate::descriptors::{CLOCK_DESCRIPTOR, CONFIG_DESCRIPTOR, PROFILES_DESCRIPTOR};
use crate::document_io::{
    load_json_document, load_toml_document, save_json_document, save_toml_document,
};
use crate::error::StorageError;
use crate::layout::StorageLayout;
use crate::profile_document::{ProfilesDocument, UserProfile};
use crate::state_documents::{RecentFilesDocument, SessionsDocument, StateDocument};
use crate::trash_document::TrashDocument;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageManager {
    pub(crate) layout: StorageLayout,
}

impl StorageManager {
    pub fn open(app_paths: AppPaths) -> Result<StorageOpen, StorageError> {
        let layout = StorageLayout::from_app_paths(&app_paths);
        let manager = Self { layout };
        let report = manager.initialize()?;

        Ok(StorageOpen { manager, report })
    }

    pub fn open_from_platform(platform: &dyn Platform) -> Result<StorageOpen, StorageError> {
        let app_paths = platform
            .app_paths()
            .map_err(|error| StorageError::Platform {
                message: error.to_string(),
            })?;

        Self::open(app_paths)
    }

    pub fn from_layout(layout: StorageLayout) -> Self {
        Self { layout }
    }

    pub fn layout(&self) -> &StorageLayout {
        &self.layout
    }

    pub fn load_config(&self) -> Result<StorageConfig, StorageError> {
        if let Some(store) = crate::session_store::session_config_store(&self.layout.config_path) {
            return store.load_config();
        }
        let mut config: StorageConfig =
            load_toml_document(&self.layout.config_path, CONFIG_DESCRIPTOR.name)?;
        config.normalize();
        Ok(config)
    }

    pub fn save_config(&self, config: &StorageConfig) -> Result<(), StorageError> {
        if let Some(store) = crate::session_store::session_config_store(&self.layout.config_path) {
            return store.save_config(config);
        }
        let mut config = config.clone();
        config.normalize();
        save_toml_document(&self.layout.config_path, CONFIG_DESCRIPTOR.name, &config)
    }

    pub fn load_profile(&self, username: &str) -> Result<Option<UserProfile>, StorageError> {
        let mut document: ProfilesDocument =
            load_json_document(&self.layout.profiles_path, PROFILES_DESCRIPTOR.name)?;
        Ok(document.profiles.remove(username).map(|mut profile| {
            profile.system_status_dashboard.normalize();
            profile
        }))
    }

    pub fn update_profile(
        &self,
        username: &str,
        update: impl FnOnce(&mut UserProfile),
    ) -> Result<UserProfile, StorageError> {
        let path = &self.layout.profiles_path;
        let mut document: ProfilesDocument = load_json_document(path, PROFILES_DESCRIPTOR.name)?;
        let profile = document.profiles.entry(username.to_owned()).or_default();
        update(profile);
        profile.system_status_dashboard.normalize();
        let saved = profile.clone();
        save_json_document(path, PROFILES_DESCRIPTOR.name, &document)?;
        Ok(saved)
    }

    pub fn load_state(&self) -> Result<StateDocument, StorageError> {
        load_json_document(&self.layout.state_path, "state")
    }

    pub fn save_state(&self, state: &StateDocument) -> Result<(), StorageError> {
        save_json_document(&self.layout.state_path, "state", state)
    }

    pub fn load_recent_files(&self) -> Result<RecentFilesDocument, StorageError> {
        load_json_document(&self.layout.recent_files_path, "recent-files")
    }

    pub fn load_recent(&self) -> Result<RecentFilesDocument, StorageError> {
        self.load_recent_files()
    }

    pub fn save_recent_files(
        &self,
        recent_files: &RecentFilesDocument,
    ) -> Result<(), StorageError> {
        save_json_document(&self.layout.recent_files_path, "recent-files", recent_files)
    }

    pub fn save_recent(&self, recent_files: &RecentFilesDocument) -> Result<(), StorageError> {
        self.save_recent_files(recent_files)
    }

    pub fn load_sessions(&self) -> Result<SessionsDocument, StorageError> {
        load_json_document(&self.layout.sessions_path, "sessions")
    }

    pub fn save_sessions(&self, sessions: &SessionsDocument) -> Result<(), StorageError> {
        save_json_document(&self.layout.sessions_path, "sessions", sessions)
    }

    pub fn load_clock(&self) -> Result<ClockDocument, StorageError> {
        load_json_document(&self.layout.clock_path, CLOCK_DESCRIPTOR.name)
    }

    pub fn save_clock(&self, clock: &ClockDocument) -> Result<(), StorageError> {
        save_json_document(&self.layout.clock_path, CLOCK_DESCRIPTOR.name, clock)
    }

    pub fn load_trash(&self) -> Result<TrashDocument, StorageError> {
        load_json_document(&self.layout.trash_manifest_path, "trash")
    }

    pub fn save_trash(&self, trash: &TrashDocument) -> Result<(), StorageError> {
        save_json_document(&self.layout.trash_manifest_path, "trash", trash)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StorageOpen {
    pub manager: StorageManager,
    pub report: StorageLoadReport,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StorageLoadReport {
    pub warnings: Vec<String>,
    pub created_files: Vec<PathBuf>,
    pub migrated_files: Vec<PathBuf>,
    pub recovered_files: Vec<RecoveredFile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveredFile {
    pub original_path: PathBuf,
    pub recovered_path: PathBuf,
}
