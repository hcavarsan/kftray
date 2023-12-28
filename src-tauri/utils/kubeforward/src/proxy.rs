use crate::{Target, TargetSelector, Port};
use crate::port_forward::PortForward;
use crate::kubecontext::create_client_with_specific_context;
use k8s_openapi::api::core::v1::Pod;
use kube::{api::Api, Client};
use serde_json::json;
use kube_runtime::wait::conditions;
use std::time::{SystemTime, UNIX_EPOCH};
use rand::{distributions::Alphanumeric, Rng};

#[tauri::command]
pub async fn deploy_and_forward_pod(
    context_name: Option<String>,
    namespace: &str,
    local_port: u16,
    remote_port: u16,
    remote_address: &str,
    protocol: &str,
) -> Result<(), String> {
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| e.to_string())?.as_secs();

	let random_string: String = rand::thread_rng()
    .sample_iter(&Alphanumeric)
    .take(6)
    .map(|b| char::from(b).to_ascii_lowercase())
    .collect();

    let hashed_name = format!("kftray-forward-{}-{}", timestamp, random_string);

    let client = match &context_name {
        Some(ref context) => create_client_with_specific_context(context).await.map_err(|e| e.to_string())?,
        None => Client::try_default().await.map_err(|e| e.to_string())?,
    };
    let pod_manifest = json!({
        "apiVersion": "v1",
        "kind": "Pod",
        "metadata": {
            "name": hashed_name,
            "labels": {
                "app": hashed_name,
            }
        },
        "spec": {
            "containers": [{
                "name": hashed_name,
                "image": "ghcr.io/dlemel8/tunneler-server:main",
                "env": [
                    {"name": "LOCAL_PORT", "value": local_port.to_string()},
                    {"name": "REMOTE_PORT", "value": remote_port.to_string()},
                    {"name": "REMOTE_ADDRESS", "value": remote_address},
                    {"name": "TUNNELED_TYPE", "value": protocol}
                ],
                "args": [protocol],
            }],
        }
    });


    let pod: Pod = serde_json::from_value(pod_manifest).map_err(|e| e.to_string())?;
    let pods: Api<Pod> = Api::namespaced(client.clone(), namespace);
    pods.create(&kube::api::PostParams::default(), &pod).await.map_err(|e| e.to_string())?;
    kube_runtime::wait::await_condition(pods.clone(), &hashed_name, conditions::is_pod_running()).await.map_err(|e| e.to_string())?;


	let target = Target::new(
		TargetSelector::ServiceName(hashed_name.clone()),
		Port::Number(remote_port as i32),
		namespace.to_owned(),
	);


    let port_forward = PortForward::new(target, Some(local_port), context_name.clone()).await.map_err(|e| e.to_string())?;
    port_forward.port_forward().await.map_err(|e| e.to_string())?;


    Ok(())
}
