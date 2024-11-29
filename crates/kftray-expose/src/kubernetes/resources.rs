use std::time::Duration;

use k8s_openapi::api::core::v1::{
    Pod,
    Secret,
    Service,
};
use kube::{
    api::{
        Api,
        PostParams,
    },
    Client as KubeClient,
};
use log::info;
use serde_json::json;
use tokio::time::sleep;

use crate::{
    config::*,
    error::*,
};

pub struct KubernetesManager {
    client: KubeClient,
    namespace: String,
}

impl KubernetesManager {
    pub async fn new(namespace: String) -> TunnelResult<Self> {
        let client = KubeClient::try_default().await?;
        Ok(Self { client, namespace })
    }

    pub async fn setup_resources(&self, config: &TunnelConfig) -> TunnelResult<()> {
        info!(
            "Creating Kubernetes resources in namespace: {}",
            self.namespace
        );

        self.create_ssh_secret(config).await?;
        self.create_service().await?;
        self.create_pod(config).await?;
        self.wait_for_pod_ready(config).await?;

        Ok(())
    }

    async fn create_ssh_secret(&self, config: &TunnelConfig) -> TunnelResult<()> {
        let secrets: Api<Secret> = Api::namespaced(self.client.clone(), &self.namespace);

        // Read SSH public key
        let pub_key_path = config.ssh_key_path.with_extension("pub");
        let pub_key = std::fs::read_to_string(pub_key_path)?;

        let secret = serde_json::from_value(serde_json::json!({
            "apiVersion": "v1",
            "kind": "Secret",
            "metadata": {
                "name": "kftray-server"
            },
            "type": "Opaque",
            "stringData": {
                "authorized_keys": pub_key
            }
        }))
        .map_err(|e| TunnelError::Other(e.into()))?;

        secrets.create(&PostParams::default(), &secret).await?;
        Ok(())
    }

    async fn create_service(&self) -> TunnelResult<()> {
        let services: Api<Service> = Api::namespaced(self.client.clone(), &self.namespace);
        let service = serde_json::from_value(serde_json::json!({
            "apiVersion": "v1",
            "kind": "Service",
            "metadata": {
                "name": "kftray-server"
            },
            "spec": {
                "selector": {
                    "app": "kftray-server"
                },
                "ports": [
                    {
                        "name": "ssh",
                        "port": 2222,
                        "targetPort": 2222
                    },
                    {
                        "name": "proxy",
                        "port": 8085,
                        "targetPort": 8085
                    }
                ]
            }
        }))
        .map_err(|e| TunnelError::Other(e.into()))?;

        services.create(&PostParams::default(), &service).await?;
        Ok(())
    }

    async fn create_pod(&self, config: &TunnelConfig) -> TunnelResult<()> {
        let pods: Api<Pod> = Api::namespaced(self.client.clone(), &self.namespace);

        let env_vars = vec![
            json!({"name": "PROXY_TYPE", "value": "ssh"}),
            json!({"name": "LOCAL_PORT", "value": config.local_port.to_string()}),
            json!({"name": "REMOTE_PORT", "value": config.remote_port.to_string()}),
            json!({"name": "REMOTE_ADDRESS", "value": "0.0.0.0"}),
            json!({"name": "RUST_LOG", "value": "trace"}),
            json!({"name": "SSH_AUTH", "value": "true"}),
            json!({
                "name": "SSH_AUTHORIZED_KEYS",
                "valueFrom": {
                    "secretKeyRef": {
                        "name": "kftray-server",
                        "key": "authorized_keys"
                    }
                }
            }),
        ];

        let pod = serde_json::from_value(serde_json::json!({
            "apiVersion": "v1",
            "kind": "Pod",
            "metadata": {
                "name": "kftray-server",
                "labels": {
                    "app": "kftray-server"
                }
            },
            "spec": {
                "containers": [{
                    "name": "kftray-server",
                    "image": "kftray-server:latest",
                    "imagePullPolicy": "Never",
                    "ports": [
                        {"containerPort": 2222, "name": "ssh"},
                        {"containerPort": 8085, "name": "proxy"}
                    ],
                    "env": env_vars,
                    "volumeMounts": [{
                        "name": "ssh-keys",
                        "mountPath": "/etc/ssh/keys",
                        "readOnly": true
                    }],
                    "securityContext": {
                        "capabilities": {
                            "add": ["NET_BIND_SERVICE"]
                        }
                    }
                }],
                "volumes": [{
                    "name": "ssh-keys",
                    "secret": {
                        "secretName": "kftray-server",
                        "defaultMode": 0o600
                    }
                }]
            }
        }))
        .map_err(|e| TunnelError::Other(e.into()))?;

        pods.create(&PostParams::default(), &pod).await?;
        Ok(())
    }

    async fn wait_for_pod_ready(&self, config: &TunnelConfig) -> TunnelResult<()> {
        let pods: Api<Pod> = Api::namespaced(self.client.clone(), &self.namespace);
        let start = std::time::Instant::now();

        while start.elapsed() < config.pod_ready_timeout {
            if let Ok(pod) = pods.get("kftray-server").await {
                if let Some(status) = &pod.status {
                    let phase = status.phase.as_deref().unwrap_or_default();
                    match phase {
                        "Failed" | "Unknown" => {
                            return Err(TunnelError::PodNotReady(format!(
                                "Pod is in {} state",
                                phase
                            )));
                        }
                        "Running" => {
                            if let Some(conditions) = &status.conditions {
                                if conditions
                                    .iter()
                                    .any(|c| c.type_ == "Ready" && c.status == "True")
                                {
                                    return Ok(());
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            sleep(Duration::from_secs(1)).await;
        }

        Err(TunnelError::PodNotReady(
            "Pod failed to become ready within timeout".to_string(),
        ))
    }
}
