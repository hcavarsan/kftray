/// Configuration settings for a proxy instance
#[derive(Debug, Clone)]
pub struct ProxyConfig {
    /// Host address of the target server to proxy to
    pub target_host: String,
    /// Port number of the target server
    pub target_port: u16,
    /// Local port number the proxy listens on
    pub proxy_port: u16,
    /// Type of proxy protocol (TCP or UDP)
    pub proxy_type: ProxyType,
    /// Simplified SSH auth fields
    pub ssh_auth_enabled: bool,
    pub ssh_authorized_keys: Option<Vec<String>>,
}

/// Builder pattern implementation for creating ProxyConfig instances
#[derive(Default)]
pub struct ProxyConfigBuilder {
    target_host: Option<String>,
    target_port: Option<u16>,
    proxy_port: Option<u16>,
    proxy_type: Option<ProxyType>,
    ssh_auth_enabled: Option<bool>,
    ssh_authorized_keys: Option<Vec<String>>,
}

impl ProxyConfigBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn target_host(mut self, host: String) -> Self {
        self.target_host = Some(host);
        self
    }

    pub fn target_port(mut self, port: u16) -> Self {
        self.target_port = Some(port);
        self
    }

    pub fn proxy_port(mut self, port: u16) -> Self {
        self.proxy_port = Some(port);
        self
    }

    pub fn proxy_type(mut self, proxy_type: ProxyType) -> Self {
        self.proxy_type = Some(proxy_type);
        self
    }

    pub fn ssh_auth_enabled(mut self, enabled: bool) -> Self {
        self.ssh_auth_enabled = Some(enabled);
        self
    }

    pub fn ssh_authorized_keys(mut self, keys: Option<Vec<String>>) -> Self {
        self.ssh_authorized_keys = keys;
        self
    }

    pub fn build(self) -> Result<ProxyConfig, String> {
        let target_host = self
            .target_host
            .ok_or_else(|| "target_host is required".to_string())?;
        let target_port = self
            .target_port
            .ok_or_else(|| "target_port is required".to_string())?;
        let proxy_port = self
            .proxy_port
            .ok_or_else(|| "proxy_port is required".to_string())?;
        let proxy_type = self
            .proxy_type
            .ok_or_else(|| "proxy_type is required".to_string())?;

        let ssh_auth_enabled = self.ssh_auth_enabled.unwrap_or(false);
        let ssh_authorized_keys = if ssh_auth_enabled {
            self.ssh_authorized_keys
        } else {
            None
        };

        Ok(ProxyConfig {
            target_host,
            target_port,
            proxy_port,
            proxy_type,
            ssh_auth_enabled,
            ssh_authorized_keys,
        })
    }
}

impl ProxyConfig {
    pub fn builder() -> ProxyConfigBuilder {
        ProxyConfigBuilder::new()
    }
}

/// Supported proxy protocol types
#[derive(Debug, Clone)]
pub enum ProxyType {
    /// TCP proxy mode
    Tcp,
    /// UDP proxy mode
    Udp,
    /// SSH proxy mode
    Ssh,
}
