/// Resources created for expose
#[derive(Debug)]
pub struct ExposeResources {
    pub deployment_name: String,
    pub service_name: String,
    pub ingress_name: Option<String>,
    pub pod_ip: String,
    pub pod_name: String,
    /// Exactly what this attempt created, by name and UID. A later rollback
    /// deletes these rather than everything sharing the configuration id, which
    /// another installation can also be using.
    pub owned: Vec<crate::expose::kubernetes::CreatedResource>,
}
