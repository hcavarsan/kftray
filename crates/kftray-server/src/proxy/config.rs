#[derive(Debug, Clone)]
pub struct ProxyConfig {
    pub target_host: String,
    pub target_port: u16,
    pub proxy_port: u16,
    pub proxy_type: ProxyType,
}

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

#[derive(Debug, Clone)]
pub enum ProxyType {
    Http,
    Tcp,
    Udp,
}
