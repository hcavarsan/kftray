//! Core functionality for external service interactions
//!
//! This module provides implementations for interacting with external services:
//! - GitHub API client for configuration management
//! - Kubernetes client for cluster operations and port forwarding

pub mod github;
//pub mod kubernetes;

pub use github::GithubClient;
//pub use kubernetes::{KubernetesClient, PortForwardManager};
