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

impl CustomResponse {
    pub fn failed(&self) -> bool {
        self.status != 0
    }
}

/// Collapses a batch of per-configuration responses into a single error.
///
/// The start and stop batch commands report one result per configuration, so an
/// all-failed batch still returns `Ok`; every consumer has to inspect the
/// statuses rather than the outer result.
pub fn batch_failure(responses: &[CustomResponse]) -> Result<(), String> {
    let failures: Vec<&str> = responses
        .iter()
        .filter(|response| response.failed())
        .map(|response| response.stderr.as_str())
        .collect();
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}
