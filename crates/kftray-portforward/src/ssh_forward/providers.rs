use std::sync::Arc;

use kftray_commons::models::ssh_provider::{
    GcpBastionProvider,
    SshProvider,
    SshProviderKind,
};
use kftray_ssh::SshRoute;
use kftray_ssh::gcp::{
    GcpContext,
    GcpInstance,
};

/// Provider state shared by every forward, built on first use.
#[derive(Default)]
pub(crate) struct ProviderContexts {
    gcp: tokio::sync::Mutex<Option<Arc<GcpContext>>>,
}

impl ProviderContexts {
    async fn gcp(&self) -> Result<Arc<GcpContext>, String> {
        let mut gcp = self.gcp.lock().await;
        if let Some(context) = gcp.as_ref() {
            return Ok(Arc::clone(context));
        }
        // Errors are not cached: the user may log in and try again.
        let context = Arc::new(GcpContext::from_env().map_err(|e| e.to_string())?);
        *gcp = Some(Arc::clone(&context));
        Ok(context)
    }

    /// Forgets every context, so the next use reads the environment again.
    #[cfg(test)]
    pub(crate) async fn clear(&self) {
        *self.gcp.lock().await = None;
    }
}

fn gcp_instance(provider: &GcpBastionProvider) -> GcpInstance {
    GcpInstance {
        project: provider.project.clone(),
        zone: provider.zone.clone(),
        instance: provider.instance.clone(),
        interface: provider.interface.clone(),
    }
}

/// The only place kftray matches on the SSH provider type.
pub(crate) async fn build_route(
    provider: &SshProvider, contexts: &ProviderContexts,
) -> Result<SshRoute, String> {
    match provider.kind()? {
        SshProviderKind::GcpBastion(bastion) => {
            kftray_ssh::gcp::bastion_route(&contexts.gcp().await?, &gcp_instance(bastion))
                .await
                .map_err(|e| e.to_string())
        }
    }
}
