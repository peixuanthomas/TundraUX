use crate::schema::{SCHEMA_VERSION, StorageFormat};

pub(crate) const TRASH_DIR_NAME: &str = "trash";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageDescriptor {
    pub name: &'static str,
    pub file_name: &'static str,
    pub format: StorageFormat,
    pub schema_version: u32,
}

pub const CONFIG_DESCRIPTOR: StorageDescriptor = StorageDescriptor {
    name: "config",
    file_name: "config.toml",
    format: StorageFormat::Toml,
    schema_version: SCHEMA_VERSION,
};

pub const CLOCK_DESCRIPTOR: StorageDescriptor = StorageDescriptor {
    name: "clock",
    file_name: "clock.v2.json",
    format: StorageFormat::VersionedJson,
    schema_version: SCHEMA_VERSION,
};

pub(crate) const PROFILES_DESCRIPTOR: StorageDescriptor = StorageDescriptor {
    name: "profiles",
    file_name: "profiles.v1.json",
    format: StorageFormat::VersionedJson,
    schema_version: SCHEMA_VERSION,
};

pub const VERSIONED_JSON_DESCRIPTORS: &[StorageDescriptor] = &[
    PROFILES_DESCRIPTOR,
    StorageDescriptor {
        name: "state",
        file_name: "state.v1.json",
        format: StorageFormat::VersionedJson,
        schema_version: SCHEMA_VERSION,
    },
    StorageDescriptor {
        name: "recent-files",
        file_name: "recent-files.v1.json",
        format: StorageFormat::VersionedJson,
        schema_version: SCHEMA_VERSION,
    },
    StorageDescriptor {
        name: "sessions",
        file_name: "sessions.v1.json",
        format: StorageFormat::VersionedJson,
        schema_version: SCHEMA_VERSION,
    },
    CLOCK_DESCRIPTOR,
    StorageDescriptor {
        name: "trash",
        file_name: "trash.v1.json",
        format: StorageFormat::VersionedJson,
        schema_version: SCHEMA_VERSION,
    },
];
