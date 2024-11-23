#[derive(Debug, Clone)]
pub struct ProxyConfig {
    pub target_host: String,
    pub target_port: u16,
    pub proxy_port: u16,
    pub proxy_type: ProxyType,
}

#[derive(Debug, Clone)]
pub enum ProxyType {
    Http,
    Tcp,
    Udp,
}

impl ProxyConfig {
    pub fn new(
        target_host: String, target_port: u16, proxy_port: u16, proxy_type: ProxyType,
    ) -> Self {
        Self {
            target_host,
            target_port,
            proxy_port,
            proxy_type,
        }
    }
}
