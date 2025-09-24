use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;

use crate::models::{
    ActionContext,
    ShortcutError,
    ShortcutResult,
};

#[async_trait]
pub trait ActionHandler: Send + Sync {
    async fn execute(&self, context: &ActionContext) -> ShortcutResult<()>;
    fn action_type(&self) -> &str;
    fn description(&self) -> &str;
}

#[derive(Clone)]
pub struct ActionRegistry {
    handlers: HashMap<String, Arc<dyn ActionHandler>>,
    shortcut_definitions: HashMap<i64, crate::models::ShortcutDefinition>,
}

impl Default for ActionRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ActionRegistry {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
            shortcut_definitions: HashMap::new(),
        }
    }

    pub fn register_handler(&mut self, handler: Arc<dyn ActionHandler>) {
        let action_type = handler.action_type().to_string();
        self.handlers.insert(action_type, handler);
    }

    pub async fn execute_action(
        &self, action_type: &str, context: &ActionContext,
    ) -> ShortcutResult<()> {
        match self.handlers.get(action_type) {
            Some(handler) => handler.execute(context).await,
            None => Err(ShortcutError::ActionExecutionFailed(format!(
                "Unknown action type: {}",
                action_type
            ))),
        }
    }

    pub async fn execute_by_id(
        &self, shortcut_id: i64, context: &ActionContext,
    ) -> ShortcutResult<()> {
        log::info!("Executing action for shortcut ID: {}", shortcut_id);

        let definition = self.shortcut_definitions.get(&shortcut_id).ok_or_else(|| {
            ShortcutError::ActionExecutionFailed(format!(
                "No shortcut definition found for ID: {}",
                shortcut_id
            ))
        })?;

        let mut action_context = context.clone();
        action_context.action_data = definition.action_data.clone();
        action_context.config_id = definition.config_id;

        self.execute_action(&definition.action_type, &action_context)
            .await
    }

    pub fn get_handler(&self, action_type: &str) -> Option<Arc<dyn ActionHandler>> {
        self.handlers.get(action_type).cloned()
    }

    pub fn list_actions(&self) -> Vec<(&str, &str)> {
        self.handlers
            .values()
            .map(|handler| (handler.action_type(), handler.description()))
            .collect()
    }

    pub fn register_shortcut_definition(&mut self, definition: crate::models::ShortcutDefinition) {
        if let Some(id) = definition.id {
            self.shortcut_definitions.insert(id, definition);
        }
    }

    pub fn unregister_shortcut_definition(&mut self, shortcut_id: i64) {
        self.shortcut_definitions.remove(&shortcut_id);
    }
}

type CustomActionHandler = Arc<dyn Fn(&ActionContext) -> ShortcutResult<()> + Send + Sync>;

pub struct CustomAction {
    action_type: String,
    description: String,
    handler: CustomActionHandler,
}

impl CustomAction {
    pub fn new<F>(action_type: String, description: String, handler: F) -> Self
    where
        F: Fn(&ActionContext) -> ShortcutResult<()> + Send + Sync + 'static,
    {
        Self {
            action_type,
            description,
            handler: Arc::new(handler),
        }
    }
}

#[async_trait]
impl ActionHandler for CustomAction {
    async fn execute(&self, context: &ActionContext) -> ShortcutResult<()> {
        (self.handler)(context)
    }

    fn action_type(&self) -> &str {
        &self.action_type
    }

    fn description(&self) -> &str {
        &self.description
    }
}

pub fn create_default_registry() -> ActionRegistry {
    ActionRegistry::new()
}
