use serde::{
    Deserialize,
    Serialize,
};

#[derive(Clone, Deserialize, Serialize, Debug)]
pub struct AppSettings {
    #[serde(default)]
    pub disconnect_timeout_minutes: u32,

    #[serde(default = "default_network_monitor")]
    pub network_monitor: bool,

    #[serde(default)]
    pub http_logs_default_enabled: bool,

    #[serde(default = "default_http_logs_max_file_size")]
    pub http_logs_max_file_size: u64,

    #[serde(default = "default_http_logs_retention_days")]
    pub http_logs_retention_days: u64,

    #[serde(default = "default_auto_update_enabled")]
    pub auto_update_enabled: bool,

    #[serde(default)]
    pub last_update_check: Option<i64>,

    #[serde(default)]
    pub ssl_enabled: bool,

    #[serde(default = "default_cert_validity")]
    pub ssl_cert_validity_days: u16,

    #[serde(default = "default_ssl_auto_regenerate")]
    pub ssl_auto_regenerate: bool,

    #[serde(default)]
    pub ssl_ca_auto_install: bool,

    #[serde(default = "default_global_shortcut")]
    pub global_shortcut: String,

    #[serde(default)]
    pub mcp_server_enabled: bool,

    #[serde(default = "default_mcp_server_port")]
    pub mcp_server_port: u16,
}

fn default_network_monitor() -> bool {
    true
}

fn default_http_logs_max_file_size() -> u64 {
    10 * 1024 * 1024 // 10MB
}

fn default_http_logs_retention_days() -> u64 {
    7
}

fn default_auto_update_enabled() -> bool {
    true
}

fn default_cert_validity() -> u16 {
    365
}

fn default_ssl_auto_regenerate() -> bool {
    true
}

fn default_global_shortcut() -> String {
    #[cfg(target_os = "macos")]
    return "Cmd+Shift+F1".to_string();
    #[cfg(not(target_os = "macos"))]
    return "Ctrl+Shift+F1".to_string();
}

fn default_mcp_server_port() -> u16 {
    3000
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            disconnect_timeout_minutes: 0,
            network_monitor: default_network_monitor(),
            http_logs_default_enabled: false,
            http_logs_max_file_size: default_http_logs_max_file_size(),
            http_logs_retention_days: default_http_logs_retention_days(),
            auto_update_enabled: default_auto_update_enabled(),
            last_update_check: None,
            ssl_enabled: false,
            ssl_cert_validity_days: default_cert_validity(),
            ssl_auto_regenerate: default_ssl_auto_regenerate(),
            ssl_ca_auto_install: false,
            global_shortcut: default_global_shortcut(),
            mcp_server_enabled: false,
            mcp_server_port: default_mcp_server_port(),
        }
    }
}

impl AppSettings {
    pub fn from_settings_manager(settings: &std::collections::HashMap<String, String>) -> Self {
        let mut app_settings = AppSettings::default();

        if let Some(value) = settings.get("disconnect_timeout_minutes") {
            app_settings.disconnect_timeout_minutes = value.parse().unwrap_or(0);
        }

        if let Some(value) = settings.get("network_monitor") {
            app_settings.network_monitor = value.parse().unwrap_or(true);
        }

        if let Some(value) = settings.get("http_logs_default_enabled") {
            app_settings.http_logs_default_enabled = value.parse().unwrap_or(false);
        }

        if let Some(value) = settings.get("http_logs_max_file_size") {
            app_settings.http_logs_max_file_size = value.parse().unwrap_or(10 * 1024 * 1024);
        }

        if let Some(value) = settings.get("http_logs_retention_days") {
            app_settings.http_logs_retention_days = value.parse().unwrap_or(7);
        }

        if let Some(value) = settings.get("auto_update_enabled") {
            app_settings.auto_update_enabled = value.parse().unwrap_or(true);
        }

        if let Some(value) = settings.get("last_update_check") {
            app_settings.last_update_check = value.parse().ok();
        }

        if let Some(value) = settings.get("ssl_enabled") {
            app_settings.ssl_enabled = value.parse().unwrap_or(false);
        }

        if let Some(value) = settings.get("ssl_cert_validity_days") {
            app_settings.ssl_cert_validity_days = value.parse().unwrap_or(365);
        }

        if let Some(value) = settings.get("ssl_auto_regenerate") {
            app_settings.ssl_auto_regenerate = value.parse().unwrap_or(true);
        }

        if let Some(value) = settings.get("ssl_ca_auto_install") {
            app_settings.ssl_ca_auto_install = value.parse().unwrap_or(false);
        }

        if let Some(value) = settings.get("global_shortcut") {
            app_settings.global_shortcut = value.clone();
        }

        if let Some(value) = settings.get("mcp_server_enabled") {
            app_settings.mcp_server_enabled = value.parse().unwrap_or(false);
        }

        if let Some(value) = settings.get("mcp_server_port") {
            app_settings.mcp_server_port = value.parse().unwrap_or(3000);
        }

        app_settings
    }

    pub fn to_settings_map(&self) -> std::collections::HashMap<String, String> {
        let mut settings = std::collections::HashMap::new();

        settings.insert(
            "disconnect_timeout_minutes".to_string(),
            self.disconnect_timeout_minutes.to_string(),
        );
        settings.insert(
            "network_monitor".to_string(),
            self.network_monitor.to_string(),
        );
        settings.insert(
            "http_logs_default_enabled".to_string(),
            self.http_logs_default_enabled.to_string(),
        );
        settings.insert(
            "http_logs_max_file_size".to_string(),
            self.http_logs_max_file_size.to_string(),
        );
        settings.insert(
            "http_logs_retention_days".to_string(),
            self.http_logs_retention_days.to_string(),
        );
        settings.insert(
            "auto_update_enabled".to_string(),
            self.auto_update_enabled.to_string(),
        );

        if let Some(timestamp) = self.last_update_check {
            settings.insert("last_update_check".to_string(), timestamp.to_string());
        }

        settings.insert("ssl_enabled".to_string(), self.ssl_enabled.to_string());
        settings.insert(
            "ssl_cert_validity_days".to_string(),
            self.ssl_cert_validity_days.to_string(),
        );
        settings.insert(
            "ssl_auto_regenerate".to_string(),
            self.ssl_auto_regenerate.to_string(),
        );
        settings.insert(
            "ssl_ca_auto_install".to_string(),
            self.ssl_ca_auto_install.to_string(),
        );
        settings.insert("global_shortcut".to_string(), self.global_shortcut.clone());
        settings.insert(
            "mcp_server_enabled".to_string(),
            self.mcp_server_enabled.to_string(),
        );
        settings.insert(
            "mcp_server_port".to_string(),
            self.mcp_server_port.to_string(),
        );

        settings
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn test_default_app_settings() {
        let settings = AppSettings::default();
        assert_eq!(settings.disconnect_timeout_minutes, 0);
        assert!(settings.network_monitor);
        assert!(!settings.http_logs_default_enabled);
        assert_eq!(settings.http_logs_max_file_size, 10 * 1024 * 1024);
        assert_eq!(settings.http_logs_retention_days, 7);
        assert!(settings.auto_update_enabled);
        assert!(settings.last_update_check.is_none());
        assert!(!settings.ssl_enabled);
        assert_eq!(settings.ssl_cert_validity_days, 365);
        assert!(settings.ssl_auto_regenerate);
    }

    #[test]
    fn test_from_settings_manager() {
        let mut settings_map = HashMap::new();
        settings_map.insert("ssl_enabled".to_string(), "true".to_string());
        settings_map.insert("ssl_cert_validity_days".to_string(), "180".to_string());
        settings_map.insert("ssl_auto_regenerate".to_string(), "false".to_string());
        settings_map.insert("network_monitor".to_string(), "false".to_string());

        let app_settings = AppSettings::from_settings_manager(&settings_map);

        assert!(app_settings.ssl_enabled);
        assert_eq!(app_settings.ssl_cert_validity_days, 180);
        assert!(!app_settings.ssl_auto_regenerate);
        assert!(!app_settings.network_monitor);
    }

    #[test]
    fn test_to_settings_map() {
        let app_settings = AppSettings {
            ssl_enabled: true,
            ssl_cert_validity_days: 180,
            ssl_auto_regenerate: false,
            ..Default::default()
        };

        let settings_map = app_settings.to_settings_map();

        assert_eq!(settings_map.get("ssl_enabled"), Some(&"true".to_string()));
        assert_eq!(
            settings_map.get("ssl_cert_validity_days"),
            Some(&"180".to_string())
        );
        assert_eq!(
            settings_map.get("ssl_auto_regenerate"),
            Some(&"false".to_string())
        );
    }
}
