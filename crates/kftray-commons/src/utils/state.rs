use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::db::operations::Database;
use crate::error::Result;
use crate::models::state::ConfigState;

#[derive(Clone)]
pub struct StateManager {
    db: Database,
    states: Arc<RwLock<HashMap<i64, bool>>>,
}

impl StateManager {
    pub async fn new(db: Database) -> Result<Self> {
        let states = Arc::new(RwLock::new(HashMap::new()));
        let manager = Self { db, states };
        manager.load_states().await?;
        Ok(manager)
    }

    async fn load_states(&self) -> Result<()> {
        let config_states = self.db.get_all_config_states().await?;
        let mut states = self.states.write().await;

        for state in config_states {
            states.insert(state.config_id, state.is_running);
        }

        Ok(())
    }

    pub async fn update_state(&self, config_id: i64, is_running: bool) -> Result<()> {
        // Create state object
        let state = ConfigState {
            id: None,
            config_id,
            is_running,
        };

        // Update database first
        self.db.update_config_state(&state).await?;

        // Then update in-memory state
        let mut states = self.states.write().await;
        states.insert(config_id, is_running);

        Ok(())
    }

    pub async fn get_state(&self, config_id: i64) -> Result<bool> {
        let states = self.states.read().await;
        Ok(*states.get(&config_id).unwrap_or(&false))
    }

    pub async fn get_all_states(&self) -> Result<Vec<ConfigState>> {
        self.db.get_all_config_states().await
    }

    pub async fn get_running_configs(&self) -> Result<Vec<i64>> {
        let states = self.states.read().await;
        Ok(states
            .iter()
            .filter(|(_, &is_running)| is_running)
            .map(|(&config_id, _)| config_id)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    #[tokio::test]
    async fn test_state_management() {
        // Use in-memory database
        let db = Database::new(":memory:".into()).await.unwrap();

        // Run migrations
        crate::db::migrations::run_migrations(&db).await.unwrap();

        let manager = StateManager::new(db.clone()).await.unwrap();

        // Create a test config first
        let config = Config::builder()
            .namespace("test")
            .protocol("TCP")
            .local_port(8080)
            .build()
            .unwrap();

        let config_id = db.save_config(&config).await.unwrap();

        // Now test state management with the valid config_id
        manager.update_state(config_id, true).await.unwrap();
        assert!(manager.get_state(config_id).await.unwrap());

        let running_configs = manager.get_running_configs().await.unwrap();
        assert_eq!(running_configs, vec![config_id]);
    }
}
