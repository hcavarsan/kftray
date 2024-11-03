//! Response models for port forwarding operations
//!
//! This module provides models for responses from port forwarding operations,
//! including custom responses and batch responses.

use std::fmt;

use serde::{
    Deserialize,
    Serialize,
};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct CustomResponse {
    pub id: Option<i64>,
    pub service: String,
    pub namespace: String,
    pub local_port: u16,
    pub remote_port: u16,
    pub context: String,
    pub stdout: String,
    pub stderr: String,
    pub status: ResponseStatus,
    pub protocol: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ResponseStatus {
    Success,
    Error,
    Pending,
    Running,
    Stopped,
}

impl CustomResponse {
    pub fn new(service: String, namespace: String) -> Self {
        Self {
            id: None,
            service,
            namespace,
            local_port: 0,
            remote_port: 0,
            context: String::new(),
            stdout: String::new(),
            stderr: String::new(),
            status: ResponseStatus::Pending,
            protocol: String::from("TCP"),
            error: None,
        }
    }

    pub fn with_error(mut self, error: impl Into<String>) -> Self {
        self.error = Some(error.into());
        self.status = ResponseStatus::Error;
        self
    }

    pub fn with_status(mut self, status: ResponseStatus) -> Self {
        self.status = status;
        self
    }

    pub fn with_ports(mut self, local_port: u16, remote_port: u16) -> Self {
        self.local_port = local_port;
        self.remote_port = remote_port;
        self
    }
}

impl fmt::Display for CustomResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} {} ({}:{} -> {}) [{}]",
            self.service,
            self.status,
            self.local_port,
            self.remote_port,
            self.namespace,
            self.context
        )
    }
}

impl fmt::Display for ResponseStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ResponseStatus::Success => write!(f, "Success"),
            ResponseStatus::Error => write!(f, "Error"),
            ResponseStatus::Pending => write!(f, "Pending"),
            ResponseStatus::Running => write!(f, "Running"),
            ResponseStatus::Stopped => write!(f, "Stopped"),
        }
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BatchResponse {
    pub responses: Vec<CustomResponse>,
    pub summary: ResponseSummary,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ResponseSummary {
    pub total: usize,
    pub successful: usize,
    pub failed: usize,
    pub pending: usize,
}

impl BatchResponse {
    pub fn new(responses: Vec<CustomResponse>) -> Self {
        let total = responses.len();
        let successful = responses
            .iter()
            .filter(|r| r.status == ResponseStatus::Success)
            .count();
        let failed = responses
            .iter()
            .filter(|r| r.status == ResponseStatus::Error)
            .count();
        let pending = responses
            .iter()
            .filter(|r| r.status == ResponseStatus::Pending)
            .count();

        Self {
            responses,
            summary: ResponseSummary {
                total,
                successful,
                failed,
                pending,
            },
        }
    }
}
