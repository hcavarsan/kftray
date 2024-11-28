use std::fs::File;
use std::io::Read;

use kftray_commons::config_model::Config;
use kftray_commons::utils::config_dir::get_pod_manifest_path;
use regex::Regex;
use serde_json::json;
use serde_json::Value;

use crate::error::Error;
pub struct ManifestLoader {
    contents: String,
}

impl ManifestLoader {
    pub async fn new() -> Result<Self, Error> {
        let manifest_path = get_pod_manifest_path().map_err(|e| Error::Config(e.to_string()))?;
        let mut file = File::open(manifest_path).map_err(Error::Io)?;
        let mut contents = String::new();
        file.read_to_string(&mut contents).map_err(Error::Io)?;

        Ok(Self { contents })
    }

    pub fn load_and_render(&self, values: &serde_json::Map<String, Value>) -> Result<Value, Error> {
        let manifest: Value = serde_json::from_str(&self.contents)?;
        let rendered = self.render_template(manifest, values)?;

        Ok(rendered)
    }

    #[allow(clippy::only_used_in_recursion)]
    fn render_template(
        &self, template: Value, values: &serde_json::Map<String, Value>,
    ) -> Result<Value, Error> {
        match template {
            Value::String(s) => {
                let re = Regex::new(r"\{\{([^}]+)\}\}")?;
                let rendered = re.replace_all(&s, |caps: &regex::Captures| {
                    let key = caps.get(1).unwrap().as_str().trim();
                    match values.get(key) {
                        Some(Value::Number(n)) => n.to_string(),
                        Some(Value::String(s)) if s.is_empty() => "0".to_string(),
                        Some(Value::String(s)) => s.to_string(),
                        Some(Value::Null) => "0".to_string(),
                        Some(v) => v.to_string(),
                        None => "0".to_string(),
                    }
                });

                // For environment variables, always return as string
                Ok(Value::String(rendered.into_owned()))
            }
            Value::Object(map) => {
                let mut new_map = serde_json::Map::new();
                for (k, v) in map {
                    new_map.insert(k, self.render_template(v, values)?);
                }
                Ok(Value::Object(new_map))
            }
            Value::Array(arr) => {
                let mut new_arr = Vec::new();
                for v in arr {
                    new_arr.push(self.render_template(v, values)?);
                }
                Ok(Value::Array(new_arr))
            }
            _ => Ok(template),
        }
    }

    pub fn create_proxy_pod_manifest(
        &self, pod_name: &str, config: &Config,
    ) -> Result<Value, Error> {
        let mut values = serde_json::Map::new();
        values.insert("hashed_name".to_string(), json!(pod_name));
        values.insert("namespace".to_string(), json!(config.namespace));
        values.insert(
            "config_id".to_string(),
            json!(config.id.unwrap_or_default().to_string()),
        );

        // Convert ports to integers
        let local_port = config.local_port.unwrap_or(0);
        let remote_port = config.remote_port.unwrap_or(0);

        values.insert("local_port".to_string(), json!(local_port));
        values.insert("remote_port".to_string(), json!(remote_port));

        // For proxy mode, use the remote address
        let remote_address = if config.workload_type.as_deref() == Some("proxy") {
            config
                .remote_address
                .clone()
                .unwrap_or_else(|| "127.0.0.1".to_string())
        } else {
            config.service.clone().unwrap_or_default()
        };

        values.insert("remote_address".to_string(), json!(remote_address));
        values.insert(
            "protocol".to_string(),
            json!(config.protocol.to_uppercase()),
        );

        let manifest: Value = serde_json::from_str(&self.contents)?;
        let rendered = self.render_template(manifest, &values)?;

        Ok(rendered)
    }
}
