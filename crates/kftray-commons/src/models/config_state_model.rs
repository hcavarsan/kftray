use serde::{
    Deserialize,
    Serialize,
};

#[derive(Clone, Deserialize, PartialEq, Serialize, Debug, Default)]
pub struct ConfigState {
    pub id: Option<i64>,
    pub config_id: i64,
    pub is_running: bool,
    pub process_id: Option<u32>,
    #[serde(default)]
    pub is_retrying: bool,
    #[serde(default)]
    pub retry_count: Option<i32>,
    #[serde(default)]
    pub last_error: Option<String>,
    /// When `process_id` started, as `process_start_identity` reports it, so a
    /// pid reused after a crash is not mistaken for the process that wrote the
    /// row. `None` for rows written by versions that recorded none.
    #[serde(default)]
    pub process_start: Option<i64>,
}

impl ConfigState {
    pub fn new(config_id: i64, is_running: bool) -> Self {
        Self {
            id: None,
            config_id,
            is_running,
            process_id: Some(std::process::id()),
            is_retrying: false,
            retry_count: None,
            last_error: None,
            process_start: None,
        }
    }

    pub fn new_without_process(config_id: i64, is_running: bool) -> Self {
        Self {
            id: None,
            config_id,
            is_running,
            process_id: None,
            is_retrying: false,
            retry_count: None,
            last_error: None,
            process_start: None,
        }
    }
}
