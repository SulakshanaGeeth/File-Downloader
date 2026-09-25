use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    Queued,
    Downloading,
    Paused,
    Completed,
    Failed,
    Cancelled,
}

/// Actions reserved by the shared contract; dispatch arrives in a later milestone.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Action {
    Pause,
    Resume,
    Cancel,
    Retry,
    Remove,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Direction {
    Up,
    Down,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Theme {
    System,
    Light,
    Dark,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Download {
    pub id: String,
    pub url: String,
    pub file_name: String,
    pub destination: String,
    pub status: Status,
    pub downloaded_bytes: u64,
    pub total_bytes: Option<u64>,
    pub speed_bps: u64,
    pub eta_seconds: Option<u64>,
    pub created_at: u64,
    pub queue_order: u64,
    pub error: Option<String>,
    pub connections: u32,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub download_dir: String,
    pub max_concurrent: u32,
    pub connections_per_download: u32,
    pub speed_limit_bps: u64,
    pub theme: Theme,
    pub notifications: bool,
}

impl Settings {
    pub fn new(download_dir: impl Into<String>) -> Self {
        Self {
            download_dir: download_dir.into(),
            max_concurrent: 3,
            connections_per_download: 4,
            speed_limit_bps: 0,
            theme: Theme::System,
            notifications: true,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub downloads: Vec<Download>,
    pub settings: Settings,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddRequest {
    pub url: String,
    #[serde(default)]
    pub file_name: Option<String>,
    #[serde(default)]
    pub destination: Option<String>,
}
