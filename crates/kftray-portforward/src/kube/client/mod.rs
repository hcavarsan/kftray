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
/// goes through this so the two sides are canonicalized the same way.
pub fn cluster_identity(url: &http::Uri) -> String {
    url.to_string()
}
