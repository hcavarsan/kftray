use serde::{
    Deserialize,
    Serialize,
};

#[derive(Clone, Deserialize, PartialEq, Serialize, Debug)]
pub struct HttpLogsConfig {
    pub config_id: i64,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_max_file_size")]
    pub max_file_size: u64,
    #[serde(default = "default_retention_days")]
    pub retention_days: u64,
    #[serde(default = "default_auto_cleanup")]
    pub auto_cleanup: bool,
}

impl Default for HttpLogsConfig {
    fn default() -> Self {
        HttpLogsConfig {
            config_id: 0,
            enabled: false,
            max_file_size: default_max_file_size(),
            retention_days: default_retention_days(),
            auto_cleanup: default_auto_cleanup(),
        }
    }
}

impl HttpLogsConfig {
    pub fn new(config_id: i64) -> Self {
        HttpLogsConfig {
            config_id,
            ..Default::default()
        }
    }
}

fn default_max_file_size() -> u64 {
    10 * 1024 * 1024 // 10MB
}

fn default_retention_days() -> u64 {
    7 // 7 days
}

fn default_auto_cleanup() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_http_logs_config_default() {
        let config = HttpLogsConfig::default();

        assert_eq!(config.config_id, 0);
        assert!(!config.enabled);
        assert_eq!(config.max_file_size, 10 * 1024 * 1024);
        assert_eq!(config.retention_days, 7);
        assert!(config.auto_cleanup);
    }

    #[test]
    fn test_http_logs_config_new() {
        let config = HttpLogsConfig::new(123);

        assert_eq!(config.config_id, 123);
        assert!(!config.enabled);
        assert_eq!(config.max_file_size, 10 * 1024 * 1024);
        assert_eq!(config.retention_days, 7);
        assert!(config.auto_cleanup);
    }

    #[test]
    fn test_http_logs_config_serde() {
        let config = HttpLogsConfig {
            config_id: 456,
            enabled: true,
            max_file_size: 5 * 1024 * 1024,
            retention_days: 14,
            auto_cleanup: false,
        };

        let json = serde_json::to_string(&config).unwrap();
        let deserialized: HttpLogsConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(config, deserialized);
    }

    #[test]
    fn test_http_logs_config_partial_json() {
        let json = r#"{"config_id": 789, "enabled": true}"#;
        let config: HttpLogsConfig = serde_json::from_str(json).unwrap();

        assert_eq!(config.config_id, 789);
        assert!(config.enabled);
        assert_eq!(config.max_file_size, 10 * 1024 * 1024);
        assert_eq!(config.retention_days, 7);
        assert!(config.auto_cleanup);
    }
}
