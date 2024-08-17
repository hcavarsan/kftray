
use anyhow::Context;
pub use k8s_openapi::api::core::v1 as vx;
use k8s_openapi::apimachinery::pkg::util::intstr::IntOrString;
use kube::ResourceExt;
use vx::Pod;

