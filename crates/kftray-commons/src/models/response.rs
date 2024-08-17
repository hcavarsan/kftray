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
