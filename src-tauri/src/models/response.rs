#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct CustomResponse {
    pub id: Option<i64>,
    pub service: String,
    pub namespace: String,
    pub local_port: u16,
    pub remote_port: u16,
    pub context: String,
    pub stdout: String,
    pub stderr: String,
    pub status: i32,
    pub protocol: String,
}

#[allow(clippy::too_many_arguments)]
impl CustomResponse {
    pub fn new(
        id: Option<i64>,
        service: String,
        namespace: String,
        local_port: u16,
        remote_port: u16,
        context: String,
        stdout: String,
        stderr: String,
        status: i32,
        protocol: String,
    ) -> Self {
        CustomResponse {
            id,
            service,
            namespace,
            local_port,
            remote_port,
            context,
            stdout,
            stderr,
            status,
            protocol,
        }
    }
}
