use std::fs::File;
use std::io::Read;
use std::sync::Arc;

use kftray_commons::config_model::Config;
use kftray_commons::utils::config_dir::get_pod_manifest_path;
use lazy_static::lazy_static;
use parking_lot::RwLock;
use regex::Regex;
use serde_json::{
    json,
    Map,
    Value,
};

use crate::error::Error;

// New type aliases for clarity
type TemplateValues = Map<String, Value>;
type CachedManifest = Arc<RwLock<Option<Value>>>;

lazy_static! {
    static ref CACHED_MANIFEST: CachedManifest = Arc::new(RwLock::new(None));
    static ref TEMPLATE_REGEX: Regex = Regex::new(r"\{\{([^}]+)\}\}").unwrap();
}

#[derive(Clone)]
pub struct ManifestLoader {
    contents: Arc<String>,
}

impl ManifestLoader {
    pub async fn new() -> Result<Self, Error> {
        if let Some(cached) = CACHED_MANIFEST.read().as_ref() {
            return Ok(Self {
                contents: Arc::new(serde_json::to_string(cached)?),
            });
        }

        let manifest_path = get_pod_manifest_path().map_err(|e| Error::Config(e.to_string()))?;

        let mut contents = String::new();
        File::open(manifest_path)
            .map_err(Error::Io)?
            .read_to_string(&mut contents)
            .map_err(Error::Io)?;

        let parsed: Value = serde_json::from_str(&contents)?;
        *CACHED_MANIFEST.write() = Some(parsed);

        Ok(Self {
            contents: Arc::new(contents),
        })
    }

    pub fn load_and_render(&self, values: &TemplateValues) -> Result<Value, Error> {
        let manifest: Value = serde_json::from_str(&self.contents)?;
        self.render_template(manifest, values)
    }

    fn render_template(&self, template: Value, values: &TemplateValues) -> Result<Value, Error> {
        match template {
            Value::String(s) => self.render_string(&s, values),
            Value::Object(map) => self.render_object(map, values),
            Value::Array(arr) => self.render_array(arr, values),
            _ => Ok(template),
        }
    }

    fn render_string(&self, s: &str, values: &TemplateValues) -> Result<Value, Error> {
        let rendered = TEMPLATE_REGEX.replace_all(s, |caps: &regex::Captures| {
            let key = caps.get(1).unwrap().as_str().trim();
            self.get_template_value(key, values)
        });
        Ok(Value::String(rendered.into_owned()))
    }

    fn render_object(
        &self, map: Map<String, Value>, values: &TemplateValues,
    ) -> Result<Value, Error> {
        let mut new_map = Map::new();
        for (k, v) in map {
            new_map.insert(k, self.render_template(v, values)?);
        }
        Ok(Value::Object(new_map))
    }

    fn render_array(&self, arr: Vec<Value>, values: &TemplateValues) -> Result<Value, Error> {
        let new_arr = arr
            .into_iter()
            .map(|v| self.render_template(v, values))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Value::Array(new_arr))
    }

    fn get_template_value(&self, key: &str, values: &TemplateValues) -> String {
        match values.get(key) {
            Some(Value::Number(n)) => n.to_string(),
            Some(Value::String(s)) if s.is_empty() => "0".to_string(),
            Some(Value::String(s)) => s.to_string(),
            Some(Value::Null) => "0".to_string(),
            Some(v) => v.to_string(),
            None => "0".to_string(),
        }
    }

    pub fn create_proxy_pod_manifest(
        &self, pod_name: &str, config: &Config,
    ) -> Result<Value, Error> {
        let values = self.create_template_values(pod_name, config);
        self.load_and_render(&values)
    }

    fn create_template_values(&self, pod_name: &str, config: &Config) -> TemplateValues {
        let mut values = TemplateValues::new();

        // Basic metadata
        values.insert("hashed_name".to_string(), json!(pod_name));
        values.insert("namespace".to_string(), json!(config.namespace));
        values.insert(
            "config_id".to_string(),
            json!(config.id.unwrap_or_default().to_string()),
        );

        // Port configuration
        values.insert(
            "local_port".to_string(),
            json!(config.local_port.unwrap_or(0)),
        );
        values.insert(
            "remote_port".to_string(),
            json!(config.remote_port.unwrap_or(0)),
        );

        // Remote address handling
        let remote_address = match config.workload_type.as_deref() {
            Some("proxy") => config
                .remote_address
                .clone()
                .unwrap_or_else(|| "127.0.0.1".to_string()),
            _ => config.service.clone().unwrap_or_default(),
        };
        values.insert("remote_address".to_string(), json!(remote_address));

        // Protocol
        values.insert(
            "protocol".to_string(),
            json!(config.protocol.to_uppercase()),
        );

        values
    }
}
