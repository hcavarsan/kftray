use std::collections::HashMap;
use std::sync::{
    Arc,
    Mutex,
};

use serde::Serialize;
use tauri::Manager;

use crate::config::get_config;
use crate::models::config::Config;

#[derive(Clone, Debug, Serialize)]
pub struct ConfigState {
    pub config: Config,
    pub running: bool,
}

#[derive(Default)]
pub struct ConfigStates {
    pub states: Arc<Mutex<HashMap<i64, ConfigState>>>,
}

impl ConfigStates {
    pub async fn set_running(&self, app_handle: &tauri::AppHandle, id: i64, running: bool) {
        let config = match get_config(id).await {
            Ok(config) => config,
            Err(e) => {
                eprintln!("Failed to fetch config for id {}: {}", id, e);
                return;
            }
        };

        {
            let mut states = self.states.lock().unwrap();
            states.insert(
                id,
                ConfigState {
                    config: config.clone(),
                    running,
                },
            );
        }
        self.emit_state_change_event(app_handle, id, running);
    }

    fn emit_state_change_event(&self, app_handle: &tauri::AppHandle, id: i64, running: bool) {
        if let Some(state) = self.states.lock().unwrap().get(&id) {
            app_handle
                .emit_all("config_state_changed", state.clone())
                .unwrap();
            println!("Config state changed: {:?}", state.clone());
        }
        println!("Config state changed: id={}, running={}", id, running);
    }
}
