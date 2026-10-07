//! What a batch of configurations would need administrator access for, judged
//! before anything is started.
//!
//! A startup that reaches for the hosts file or a loopback alias without the
//! helper either prompts through the platform's elevation, one prompt per
//! change, or fails after it has already acquired other local resources.
//! Asking the user first lets them install the helper once, or accept the
//! prompts for this start, instead of discovering the problem from a failed
//! start. Everything here is a read: nothing is created, nothing prompts.

use kftray_commons::models::config_model::Config;
use kftray_commons::utils::hostsfile::hosts_file_writable;
use serde::Serialize;

use crate::network_utils::{
    is_address_accessible,
    is_custom_loopback_address,
};

const APP_ID: &str = "com.kftray.app";

/// A local resource the process cannot set up by itself.
#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PrivilegedResource {
    /// A domain alias line in the hosts file.
    HostsFile,
    /// A `127.x.y.z` alias on the loopback interface.
    LoopbackAddress,
}

#[derive(Serialize, Debug, Clone, PartialEq, Eq)]
pub struct PrivilegeNeed {
    pub config_id: i64,
    pub resource: PrivilegedResource,
    /// The alias or address the configuration asked for, for display.
    pub detail: String,
}

/// Everything starting `configs` would need elevation for right now. Empty
/// when the helper answers, the process is root, or nothing privileged is
/// asked for.
pub async fn preflight(configs: &[Config]) -> Vec<PrivilegeNeed> {
    if helper_is_running() || process_is_root() {
        return Vec::new();
    }
    // A missing or unsupported hosts path is a write error in its own terms,
    // not a privilege problem, so it must not trigger a privilege prompt.
    let hosts_writable = hosts_file_writable().unwrap_or(true);
    let mut needs = Vec::new();
    for config in configs {
        let Some(config_id) = config.id else {
            continue;
        };
        for (resource, detail) in
            needs_for(config, hosts_writable, loopback_aliases_need_privileges()).await
        {
            needs.push(PrivilegeNeed {
                config_id,
                resource,
                detail,
            });
        }
    }
    needs
}

/// The resources `config` would reach for on this start, given what the
/// process can already do. Mirrors the gates in `kube::start`: a domain alias
/// writes the hosts file; an auto-allocated address creates a fresh alias
/// when the helper is absent; a static custom address only needs one when it
/// is not already bound.
async fn needs_for(
    config: &Config, hosts_writable: bool, aliases_need_privileges: bool,
) -> Vec<(PrivilegedResource, String)> {
    let mut needs = Vec::new();
    // Expose configurations publish through the cluster and touch nothing
    // local that needs privileges.
    if config.workload_type.as_deref() == Some("expose") {
        return needs;
    }
    let label = config
        .alias
        .clone()
        .or_else(|| config.service.clone())
        .unwrap_or_default();
    if config.domain_enabled.unwrap_or_default() && config.service.is_some() && !hosts_writable {
        needs.push((PrivilegedResource::HostsFile, label.clone()));
    }
    if config.auto_loopback_address {
        if aliases_need_privileges {
            needs.push((PrivilegedResource::LoopbackAddress, label));
        }
    } else if let Some(address) = config.local_address.as_deref()
        && is_custom_loopback_address(address)
        && !is_address_accessible(address).await
    {
        needs.push((PrivilegedResource::LoopbackAddress, address.to_string()));
    }
    needs
}

/// Linux and Windows route the whole `127/8` block to the loopback interface,
/// so any custom address binds without an alias and
/// `ensure_loopback_address` returns before it would escalate. Only macOS
/// needs `ifconfig lo0 alias`, and with it an admin prompt, per address.
fn loopback_aliases_need_privileges() -> bool {
    cfg!(target_os = "macos")
}

fn helper_is_running() -> bool {
    use kftray_helper::client::socket_comm;
    use kftray_helper::messages::{
        RequestCommand,
        RequestResult,
    };

    let Ok(socket_path) = kftray_helper::communication::get_default_socket_path() else {
        return false;
    };
    if !socket_comm::is_socket_available(&socket_path) {
        return false;
    }
    matches!(
        socket_comm::send_request(&socket_path, APP_ID, RequestCommand::Ping),
        Ok(response) if matches!(&response.result, RequestResult::StringSuccess(s) if s == "pong")
    )
}

fn process_is_root() -> bool {
    #[cfg(target_os = "linux")]
    {
        // SAFETY: `geteuid` reads the calling process's effective uid and
        // has no preconditions.
        unsafe { libc::geteuid() == 0 }
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> Config {
        Config {
            id: Some(1),
            service: Some("svc".to_string()),
            alias: Some("svc.local".to_string()),
            namespace: "ns".to_string(),
            protocol: "tcp".to_string(),
            workload_type: Some("service".to_string()),
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn a_plain_forward_needs_nothing() {
        assert!(needs_for(&config(), false, true).await.is_empty());
    }

    #[tokio::test]
    async fn a_domain_alias_needs_the_hosts_file_only_when_it_is_not_writable() {
        let mut cfg = config();
        cfg.domain_enabled = Some(true);
        assert_eq!(
            needs_for(&cfg, false, false).await,
            vec![(PrivilegedResource::HostsFile, "svc.local".to_string())]
        );
        assert!(needs_for(&cfg, true, false).await.is_empty());
    }

    #[tokio::test]
    async fn an_auto_allocated_address_needs_an_alias_only_where_aliases_are_privileged() {
        let mut cfg = config();
        cfg.auto_loopback_address = true;
        assert_eq!(
            needs_for(&cfg, true, true).await,
            vec![(PrivilegedResource::LoopbackAddress, "svc.local".to_string())]
        );
        assert!(needs_for(&cfg, true, false).await.is_empty());
    }

    #[tokio::test]
    async fn a_custom_address_that_already_binds_needs_nothing() {
        // The gate is the bind, not the configuration: an address that is
        // reachable now will not make the start escalate.
        let mut cfg = config();
        cfg.local_address = Some("127.0.0.1".to_string());
        assert!(needs_for(&cfg, true, true).await.is_empty());
    }

    #[tokio::test]
    async fn an_expose_configuration_needs_nothing_locally() {
        let mut cfg = config();
        cfg.workload_type = Some("expose".to_string());
        cfg.domain_enabled = Some(true);
        cfg.auto_loopback_address = true;
        assert!(needs_for(&cfg, false, true).await.is_empty());
    }
}
