use std::sync::Arc;

use async_trait::async_trait;
use russh::keys::PublicKey;

use super::GcpContext;
use super::consts::SSH_PORT;
use super::oslogin::OsLoginAuth;
use crate::error::SshError;
use crate::route::{
    HostKeySource,
    RouteKey,
    SshRoute,
};
use crate::transport::iap::{
    IapTarget,
    IapTransport,
};

/// A Compute Engine VM reached through IAP.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GcpInstance {
    pub project: String,
    pub zone: String,
    pub instance: String,
    pub interface: String,
}

/// Builds the route to the sshd of `target`. Fails fast (credentials, VM
/// state, OS Login mode) before any tunnel is opened.
pub async fn bastion_route(
    ctx: &Arc<GcpContext>, target: &GcpInstance,
) -> Result<SshRoute, SshError> {
    let email = ctx.email().await?;
    let (instance, flags) = ctx.inspect(target).await?;
    let GcpInstance {
        project,
        zone,
        instance: name,
        interface,
    } = target;
    Ok(SshRoute {
        key: RouteKey::new(format!(
            "gcp-bastion:{project}/{zone}/{name}/{interface}:{email}"
        )),
        label: format!("gcp-bastion {name} ({project}/{zone})"),
        transport: Arc::new(IapTransport::new(
            IapTarget::Instance {
                project: project.clone(),
                zone: zone.clone(),
                instance: name.clone(),
                interface: interface.clone(),
                port: SSH_PORT,
            },
            ctx.endpoints.iap_ws.clone(),
            Arc::clone(&ctx.tokens),
        )),
        auth: Arc::new(OsLoginAuth::new(Arc::clone(ctx), target.clone(), email)),
        host_keys: Arc::new(GuestAttributesSource {
            ctx: Arc::clone(ctx),
            target: target.clone(),
            alias: format!("gcp:compute.{}", instance.id),
            guest_attributes: flags.guest_attributes,
        }),
    })
}

/// Host keys the VM publishes as guest attributes; without them the engine
/// trusts the key on first use.
pub(super) struct GuestAttributesSource {
    pub(super) ctx: Arc<GcpContext>,
    pub(super) target: GcpInstance,
    pub(super) alias: String,
    pub(super) guest_attributes: bool,
}

#[async_trait]
impl HostKeySource for GuestAttributesSource {
    fn alias(&self) -> String {
        self.alias.clone()
    }

    async fn trusted(&self) -> Result<Vec<PublicKey>, SshError> {
        if !self.guest_attributes {
            return Ok(Vec::new());
        }
        self.ctx
            .guest_host_keys(&self.target)
            .await
            .map_err(SshError::from)
    }
}
