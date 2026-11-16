pub(crate) mod client;
mod client_jobs;
mod client_worker;
pub(crate) mod clock;
mod server;
mod state;

pub(crate) use client::SessionServicesClient;
pub(crate) use server::run_session_services;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(crate) struct SessionProfile {
    pub appearance: Option<storage::AppearanceConfig>,
    pub dashboard: Option<storage::SystemStatusDashboardConfig>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SystemStatusRequests {
    pub active: bool,
    pub refresh: bool,
}
