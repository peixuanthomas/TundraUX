mod animation;
mod atomic_write;
mod clock_document;
mod config_document;
mod descriptors;
mod document_health;
mod document_io;
mod error;
mod initialization;
mod layout;
mod manager;
mod profile_document;
mod recovery;
mod reset;
mod schema;
mod session_store;
mod state_documents;
mod system_status_dashboard;
mod trash_document;

pub use animation::{AnimationTimingsConfig, MAX_ANIMATION_PHASE_MS, MIN_ANIMATION_PHASE_MS};
pub use clock_document::{ClockDocument, ClockEntryRecord, ClockProfile};
pub use config_document::{
    ANIMATION_SPEED_STEP_PERCENT, AccentColor, AppearanceConfig, AutoAdminPolicy, AutomationConfig,
    BorderColor, BorderColorParseError, BorderShape, CommandLineConfig,
    DEFAULT_ANIMATION_SPEED_PERCENT, DEFAULT_EDITOR_EXPLORER_OPEN_EXTENSIONS,
    DEFAULT_SCREEN_TIMEOUT_SECONDS, EditorConfig, ExplorerConfig, ExplorerDateZone,
    ExplorerSizeFormat, ExplorerSortDirection, ExplorerSortField, IconDisplayMode, LauncherConfig,
    LauncherCustomCommand, LauncherEntryRecord, LauncherExecutableKind, LauncherTerminalSettings,
    LinuxUpdateMode, MAX_ANIMATION_SPEED_PERCENT, MAX_EDITOR_EXPLORER_OPEN_EXTENSION_LEN,
    MAX_EDITOR_EXPLORER_OPEN_EXTENSIONS, MIN_ANIMATION_SPEED_PERCENT, MotionPreference,
    RuntimeLogsConfig, SYSTEM_STATUS_MAX_AVAILABLE_GIB, SYSTEM_STATUS_MAX_PERCENTAGE,
    SYSTEM_STATUS_MIN_AVAILABLE_GIB, SYSTEM_STATUS_MIN_PERCENTAGE, StatusTextAlignment,
    StorageConfig, SystemStatusConfig, TimeSyncConfig, TimeSyncSource, TopBarConfig,
    normalize_editor_explorer_open_extension, uses_default_editor_explorer_extensions,
};
pub use descriptors::{
    CLOCK_DESCRIPTOR, CONFIG_DESCRIPTOR, StorageDescriptor, VERSIONED_JSON_DESCRIPTORS,
};
pub use document_health::{
    StorageDocumentCheck, StorageDocumentKind, StorageDocumentStatus, StorageRepairReport,
};
pub use error::StorageError;
pub use layout::StorageLayout;
pub use manager::{RecoveredFile, StorageLoadReport, StorageManager, StorageOpen};
pub use profile_document::UserProfile;
pub use reset::{StorageResetReport, reset_saved_content};
pub use schema::{SCHEMA_VERSION, StorageFormat, VersionedDocument};
pub use session_store::{SessionConfigStore, install_session_config_store};
pub use state_documents::{RecentFilesDocument, SessionsDocument, StateDocument};
pub use system_status_dashboard::{
    DashboardLayout, DashboardProfile, SystemStatusDashboardConfig, SystemStatusWidgetKind,
    SystemStatusWidgetSize, WidgetPlacement,
};
pub use trash_document::{TrashDocument, TrashRecord};
