use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::schema::{SCHEMA_VERSION, VersionedDocument};
use crate::{AppearanceConfig, SystemStatusDashboardConfig};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct ProfilesDocument {
    pub schema_version: u32,
    pub profiles: BTreeMap<String, UserProfile>,
}

impl Default for ProfilesDocument {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            profiles: BTreeMap::new(),
        }
    }
}

impl VersionedDocument for ProfilesDocument {
    fn schema_version(&self) -> u32 {
        self.schema_version
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct UserProfile {
    pub appearance: AppearanceConfig,
    pub system_status_dashboard: SystemStatusDashboardConfig,
}
