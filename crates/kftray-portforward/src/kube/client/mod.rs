pub mod builder;
pub mod config;
pub mod connection;
pub mod error;
pub mod proxy;
pub mod utils;

pub use builder::create_client_with_specific_context;
pub use config::{
    ConfigExtClone,
    create_config_with_context,
    get_kubeconfig_paths_from_option,
    merge_kubeconfigs,
};
pub use connection::create_client_with_config;
pub use error::{
    KubeClientError,
    KubeResult,
};

#[derive(Clone)]
pub struct KubeConnection {
    pub client: kube::Client,
    pub cluster_url: http::Uri,
}

impl std::fmt::Debug for KubeConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("KubeConnection")
            .field("cluster_url", &self.cluster_url)
            .finish_non_exhaustive()
    }
}

/// Canonical string identity of a cluster API-server URL.
///
/// `http::Uri` implements `PartialEq<str>` as a component-wise comparison,
/// which disagrees in edge cases (default ports, trailing slashes) with
/// plain string equality on `Uri::to_string()`. Every place that compares a
/// freshly resolved connection's cluster against a previously recorded one
/// goes through this so the two sides are canonicalized the same way:
/// scheme and host are lowercased, the scheme's default port is dropped,
/// and a trailing slash on an otherwise-empty path is dropped.
pub fn cluster_identity(url: &http::Uri) -> String {
    let scheme = url.scheme_str().unwrap_or("").to_ascii_lowercase();
    let host = url.host().unwrap_or("").to_ascii_lowercase();
    let default_port = match scheme.as_str() {
        "https" => Some(443),
        "http" => Some(80),
        _ => None,
    };
    let port = url.port_u16().filter(|port| Some(*port) != default_port);
    let path = match url.path() {
        "/" => "",
        path => path,
    };

    let mut identity = String::new();
    if !scheme.is_empty() {
        identity.push_str(&scheme);
        identity.push_str("://");
    }
    identity.push_str(&host);
    if let Some(port) = port {
        identity.push(':');
        identity.push_str(&port.to_string());
    }
    identity.push_str(path);
    if let Some(query) = url.query() {
        identity.push('?');
        identity.push_str(query);
    }
    identity
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cluster_identity_drops_default_port_and_trailing_slash() {
        let with_port: http::Uri = "https://host:443/".parse().unwrap();
        let without_port: http::Uri = "https://host".parse().unwrap();

        assert_eq!(
            cluster_identity(&with_port),
            cluster_identity(&without_port)
        );
        assert_eq!(cluster_identity(&with_port), "https://host");
    }

    #[test]
    fn cluster_identity_lowercases_scheme_and_host() {
        let mixed_case: http::Uri = "HTTPS://Host.Example.COM:6443/".parse().unwrap();
        assert_eq!(
            cluster_identity(&mixed_case),
            "https://host.example.com:6443"
        );
    }

    #[test]
    fn cluster_identity_keeps_non_default_port_and_path() {
        let url: http::Uri = "http://host:9443/api".parse().unwrap();
        assert_eq!(cluster_identity(&url), "http://host:9443/api");
    }
}
