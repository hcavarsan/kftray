use anyhow::{
    Context,
    Result,
    anyhow,
};
use kftray_commons::models::config_model::Config;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{
    Value,
    json,
};

use crate::app::Desktop;

#[derive(Deserialize)]
#[serde(rename_all = "lowercase")]
enum Outcome<T> {
    Ok(T),
    Err(String),
}

impl Desktop {
    pub async fn invoke<T: DeserializeOwned>(&self, command: &str, args: Value) -> Result<T> {
        let outcome: Outcome<T> = self
            .driver
            .execute_async(include_str!("js/invoke.js"), vec![json!(command), args])
            .await
            .with_context(|| format!("run invoke {command} in the webview"))?
            .convert()
            .with_context(|| format!("decode the reply of {command}"))?;
        match outcome {
            Outcome::Ok(value) => Ok(value),
            Outcome::Err(message) => Err(anyhow!(message)).context(format!("invoke {command}")),
        }
    }

    pub async fn invoke_detached(&self, command: &str, args: Value) -> Result<()> {
        self.driver
            .execute(
                include_str!("js/invoke_detached.js"),
                vec![json!(command), args],
            )
            .await
            .with_context(|| format!("dispatch {command} in the webview"))?;
        Ok(())
    }

    pub async fn mock(&self, command: &str, value: Value) -> Result<()> {
        self.driver
            .execute(include_str!("js/mock.js"), vec![json!(command), value])
            .await
            .with_context(|| format!("mock {command}"))?;
        Ok(())
    }

    pub async fn import_configs(&self, configs: &[&Config]) -> Result<()> {
        let json = serde_json::to_string(configs).context("serialize configs")?;
        self.invoke("import_configs_cmd", json!({ "json": json }))
            .await
    }
}
