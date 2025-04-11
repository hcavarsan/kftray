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
}

/// Builder pattern implementation for creating ProxyConfig instances
#[derive(Default)]
pub struct ProxyConfigBuilder {
    target_host: Option<String>,
    target_port: Option<u16>,
    proxy_port: Option<u16>,
    proxy_type: Option<ProxyType>,
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

        Ok(ProxyConfig {
            target_host,
            target_port,
            proxy_port,
            proxy_type,
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_builder_valid() {
        let config = ProxyConfig::builder()
            .target_host("localhost".to_string())
            .target_port(8080)
            .proxy_port(9090)
            .proxy_type(ProxyType::Tcp)
            .build()
            .unwrap();

        assert_eq!(config.target_host, "localhost");
        assert_eq!(config.target_port, 8080);
        assert_eq!(config.proxy_port, 9090);
        assert!(matches!(config.proxy_type, ProxyType::Tcp));
    }

    #[test]
    fn test_config_builder_missing_fields() {
        let result = ProxyConfig::builder().build();
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "target_host is required");

        let result = ProxyConfig::builder()
            .target_host("localhost".to_string())
            .build();
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "target_port is required");

        let result = ProxyConfig::builder()
            .target_host("localhost".to_string())
            .target_port(8080)
            .build();
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "proxy_port is required");

        let result = ProxyConfig::builder()
            .target_host("localhost".to_string())
            .target_port(8080)
            .proxy_port(9090)
            .build();
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "proxy_type is required");
    }

    #[test]
    fn test_config_builder_udp() {
        let config = ProxyConfig::builder()
            .target_host("localhost".to_string())
            .target_port(8080)
            .proxy_port(9090)
            .proxy_type(ProxyType::Udp)
            .build()
            .unwrap();

        assert_eq!(config.target_host, "localhost");
        assert_eq!(config.target_port, 8080);
        assert_eq!(config.proxy_port, 9090);
        assert!(matches!(config.proxy_type, ProxyType::Udp));
    }

    #[test]
    fn test_config_builder_chaining() {
        let builder = ProxyConfig::builder()
            .target_host("first".to_string())
            .target_host("second".to_string())
            .target_port(1234)
            .target_port(5678);

        let config = builder
            .proxy_port(9090)
            .proxy_type(ProxyType::Tcp)
            .build()
            .unwrap();

        assert_eq!(config.target_host, "second");
        assert_eq!(config.target_port, 5678);
    }

    #[test]
    fn test_config_clone() {
        let config = ProxyConfig::builder()
            .target_host("localhost".to_string())
            .target_port(8080)
            .proxy_port(9090)
            .proxy_type(ProxyType::Tcp)
            .build()
            .unwrap();

        let cloned = config.clone();
        assert_eq!(config.target_host, cloned.target_host);
        assert_eq!(config.target_port, cloned.target_port);
        assert_eq!(config.proxy_port, cloned.proxy_port);
        assert!(matches!(cloned.proxy_type, ProxyType::Tcp));
    }
}
