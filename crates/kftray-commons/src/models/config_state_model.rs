use serde::{Deserialize, Serialize};

#[derive(Clone, Deserialize, PartialEq, Serialize, Debug)]
pub struct ConfigState {
    pub id: Option<i64>,
    pub config_id: i64,
    pub is_running: bool,
    pub process_id: Option<u32>,
}

impl ConfigState {
    pub fn new(config_id: i64, is_running: bool) -> Self {
        Self {
            id: None,
            config_id,
            is_running,
            process_id: Some(std::process::id()),
        }
    }

    pub fn new_without_process(config_id: i64, is_running: bool) -> Self {
        Self {
            id: None,
            config_id,
            is_running,
            process_id: None,
        }
    }
}
