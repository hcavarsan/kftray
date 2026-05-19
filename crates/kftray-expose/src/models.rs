/// Resources created for expose
#[derive(Debug)]
pub struct ExposeResources {
    pub deployment_name: String,
    pub service_name: String,
    pub ingress_name: Option<String>,
    pub pod_ip: String,
    pub pod_name: String,
}
