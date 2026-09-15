/// Resources created for expose
#[derive(Debug)]
pub struct ExposeResources {
    pub deployment_name: String,
    pub service_name: String,
    pub ingress_name: Option<String>,
    pub pod_ip: String,
    pub pod_name: String,
    /// The relay's actual pod-side port, resolved from `WEBSOCKET_PORT` (or
    /// the template default): what the tunnel's port-forward must target,
    /// which is not necessarily 9999 in a customized manifest.
    pub websocket_port: u16,
    /// Exactly what this attempt created, by name and UID. A later rollback
    /// deletes these rather than everything sharing the configuration id, which
    /// another installation can also be using.
    pub owned: Vec<crate::expose::kubernetes::CreatedResource>,
}
