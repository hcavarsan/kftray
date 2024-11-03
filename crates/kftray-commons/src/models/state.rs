//! State models for port forwarding configurations
//!
//! This module provides models for the state of port forwarding configurations,
//! including configuration state.

use serde::{
    Deserialize,
    Serialize,
};

#[derive(Clone, Deserialize, PartialEq, Serialize, Debug)]
pub struct ConfigState {
    pub id: Option<i64>,
    pub config_id: i64,
    pub is_running: bool,
}

impl ConfigState {
    pub fn new(config_id: i64) -> Self {
        Self {
            id: None,
            config_id,
            is_running: false,
        }
    }
}
