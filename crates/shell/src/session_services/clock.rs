use std::collections::{BTreeMap, HashMap, HashSet};

use chrono::{DateTime, NaiveDate, Utc};
use identity::{AuthSession, UserRole};
use serde::{Deserialize, Serialize};
use storage::{ClockEntryRecord, ClockProfile, StorageManager};
use workspace_protocol::{ServiceError, ServiceOperation, ServiceValue};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ClockAnchor {
    pub utc: Option<DateTime<Utc>>,
    pub observed_at: DateTime<Utc>,
}

impl ClockAnchor {
    pub(crate) fn current_utc(self) -> Option<DateTime<Utc>> {
        let elapsed = Utc::now()
            .signed_duration_since(self.observed_at)
            .max(chrono::TimeDelta::zero());
        self.utc.and_then(|utc| utc.checked_add_signed(elapsed))
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SharedClockProfile {
    #[serde(flatten)]
    pub profile: ClockProfile,
    #[serde(default)]
    pub alarm_dates: BTreeMap<u64, NaiveDate>,
}

#[derive(Default)]
pub(super) struct ServerClockState {
    profile_revision: u64,
    alarm_dates: HashMap<String, BTreeMap<u64, NaiveDate>>,
    anchor: Option<ClockAnchor>,
    anchor_revision: u64,
}

impl ServerClockState {
    pub(super) fn execute(
        &mut self,
        operation: ServiceOperation,
        shell: bool,
        authorization: Option<&AuthSession>,
        storage: &StorageManager,
    ) -> Result<ServiceValue, ServiceError> {
        match operation {
            ServiceOperation::ReadClockTime => versioned(self.anchor_revision, self.anchor),
            ServiceOperation::WriteClockTime { anchor } if shell => {
                self.anchor =
                    serde_json::from_value(anchor).map_err(|_| ServiceError::InvalidRequest)?;
                self.anchor_revision += 1;
                versioned(self.anchor_revision, self.anchor)
            }
            ServiceOperation::ReadClockProfile => {
                let username = authorized_clock_user(authorization)?;
                let mut document = storage.load_clock().map_err(storage_error)?;
                let profile = SharedClockProfile {
                    profile: document.profiles.remove(username).unwrap_or_default(),
                    alarm_dates: self.alarm_dates.get(username).cloned().unwrap_or_default(),
                };
                versioned(self.profile_revision, profile)
            }
            ServiceOperation::WriteClockProfile { revision, profile } => {
                let username = authorized_clock_user(authorization)?;
                if revision != self.profile_revision {
                    return Err(ServiceError::Conflict);
                }
                let SharedClockProfile {
                    profile,
                    alarm_dates,
                } = serde_json::from_value(profile).map_err(|_| ServiceError::InvalidRequest)?;
                let mut document = storage.load_clock().map_err(storage_error)?;
                let saved = document.profiles.get(username);
                let next_ids = daily_alarm_ids(&profile).collect::<HashSet<_>>();
                let existing_ids = saved
                    .into_iter()
                    .flat_map(daily_alarm_ids)
                    .collect::<HashSet<_>>();
                let stored_dates = self.alarm_dates.get(username).cloned().unwrap_or_default();
                let mut next_dates = stored_dates.clone();
                next_dates.retain(|id, _| next_ids.contains(id));
                next_dates.extend(alarm_dates.into_iter().filter(|(id, _)| {
                    next_ids.contains(id) && (shell || !existing_ids.contains(id))
                }));
                let changed_profile = saved != Some(&profile);
                if changed_profile {
                    document.profiles.insert(username.to_owned(), profile);
                    storage.save_clock(&document).map_err(storage_error)?;
                }
                if changed_profile || next_dates != stored_dates {
                    if next_dates.is_empty() {
                        self.alarm_dates.remove(username);
                    } else {
                        self.alarm_dates.insert(username.to_owned(), next_dates);
                    }
                    self.profile_revision += 1;
                }
                Ok(ServiceValue::Versioned {
                    revision: self.profile_revision,
                    value: serde_json::Value::Null,
                })
            }
            _ => Err(ServiceError::Forbidden),
        }
    }
}

fn daily_alarm_ids(profile: &ClockProfile) -> impl Iterator<Item = u64> + '_ {
    profile.entries.iter().filter_map(|entry| match entry {
        ClockEntryRecord::DailyAlarm { id, .. } => Some(*id),
        ClockEntryRecord::Countdown { .. } => None,
    })
}

fn authorized_clock_user(authorization: Option<&AuthSession>) -> Result<&str, ServiceError> {
    authorization
        .filter(|auth| auth.role != UserRole::Guest)
        .map(|auth| auth.username.as_str())
        .ok_or(ServiceError::Forbidden)
}

fn versioned(revision: u64, value: impl Serialize) -> Result<ServiceValue, ServiceError> {
    serde_json::to_value(value)
        .map(|value| ServiceValue::Versioned { revision, value })
        .map_err(|_| ServiceError::InvalidRequest)
}

fn storage_error(error: storage::StorageError) -> ServiceError {
    ServiceError::OperationFailed {
        message: serde_json::to_value(i18n::msg!(
            "shell-clock-data-could-not-be-saved-arg1",
            arg1 = error.to_string()
        ))
        .expect("clock storage message serializes"),
    }
}
