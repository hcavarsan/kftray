use crate::error::Error;

/// Apiserver version metadata used to gate on KEP-4006 (WebSocket port-forward
/// requires >= 1.30).
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct VersionInfo {
    pub major: u32,
    pub minor: u32,
    pub git_version: String,
}

impl VersionInfo {
    /// Create a new `VersionInfo`.
    pub fn new(major: u32, minor: u32, git_version: String) -> Self {
        Self {
            major,
            minor,
            git_version,
        }
    }

    pub fn supports_ws_portforward(&self) -> bool {
        (self.major, self.minor) >= (1, 30)
    }
}

fn parse_component(version_part: &str) -> Result<u32, Error> {
    let digits: String = version_part
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().map_err(|e: std::num::ParseIntError| {
        Error::Configuration(format!(
            "could not parse apiserver version component '{version_part}': {e}"
        ))
    })
}

/// Query the apiserver's `/version` endpoint.
pub async fn detect(client: &kube::Client) -> Result<VersionInfo, Error> {
    let info = client.apiserver_version().await.map_err(Error::Kube)?;

    Ok(VersionInfo {
        major: parse_component(&info.major)?,
        minor: parse_component(&info.minor)?,
        git_version: info.git_version,
    })
}

#[cfg(feature = "version-cache")]
mod cache {
    use std::sync::OnceLock;
    use std::time::{
        Duration,
        Instant,
    };

    use dashmap::DashMap;

    use super::VersionInfo;

    const VERSION_TTL: Duration = Duration::from_secs(3600);

    /// Process-wide version cache keyed by cluster URL. Opt-in via the
    /// `version-cache` feature.
    pub struct VersionCache {
        map: DashMap<String, (VersionInfo, Instant)>,
    }

    impl Default for VersionCache {
        fn default() -> Self {
            Self::new()
        }
    }

    impl VersionCache {
        pub fn new() -> Self {
            Self {
                map: DashMap::new(),
            }
        }

        pub fn get(&self, cluster_url: &str) -> Option<VersionInfo> {
            self.map.get(cluster_url).and_then(|entry| {
                let (info, cached_at) = entry.value();
                if cached_at.elapsed() < VERSION_TTL {
                    Some(info.clone())
                } else {
                    None
                }
            })
        }

        pub fn insert(&self, cluster_url: String, info: VersionInfo) {
            self.map.insert(cluster_url, (info, Instant::now()));
        }

        pub fn invalidate(&self, cluster_url: &str) {
            self.map.remove(cluster_url);
        }
    }

    static GLOBAL_VERSION_CACHE_CELL: OnceLock<VersionCache> = OnceLock::new();

    pub fn global_version_cache() -> &'static VersionCache {
        GLOBAL_VERSION_CACHE_CELL.get_or_init(VersionCache::new)
    }
}

#[cfg(feature = "version-cache")]
pub use cache::{
    VersionCache,
    global_version_cache,
};
