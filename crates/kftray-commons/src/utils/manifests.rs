use std::fs::File;
use std::io::Write;
use std::path::Path;

use crate::utils::config_dir::{
    create_config_dir, get_expose_deployment_manifest_path, get_expose_ingress_manifest_path,
    get_expose_service_manifest_path, get_proxy_deployment_manifest_path,
};

/// Default proxy deployment manifest template (Deployment-based)
/// Placeholders: {hashed_name}, {config_id}, {local_port}, {remote_port},
/// {remote_address}, {protocol}
const DEFAULT_PROXY_DEPLOYMENT: &str = r#"{
  "apiVersion": "apps/v1",
  "kind": "Deployment",
  "metadata": {
    "name": "{hashed_name}",
    "labels": {
      "app": "{hashed_name}",
      "config_id": "{config_id}"
    }
  },
  "spec": {
    "replicas": 1,
    "selector": {
      "matchLabels": {
        "app": "{hashed_name}",
        "config_id": "{config_id}"
      }
    },
    "template": {
      "metadata": {
        "labels": {
          "app": "{hashed_name}",
          "config_id": "{config_id}"
        }
      },
      "spec": {
        "containers": [{
          "name": "{hashed_name}",
          "image": "ghcr.io/hcavarsan/kftray-server:latest",
          "env": [
            {"name": "LOCAL_PORT", "value": "{local_port}"},
            {"name": "REMOTE_PORT", "value": "{remote_port}"},
            {"name": "REMOTE_ADDRESS", "value": "{remote_address}"},
            {"name": "PROXY_TYPE", "value": "{protocol}"},
            {"name": "RUST_LOG", "value": "DEBUG"}
          ],
          "resources": {
            "limits": {"cpu": "100m", "memory": "200Mi"},
            "requests": {"cpu": "100m", "memory": "100Mi"}
          }
        }]
      }
    }
  }
}"#;

/// Default expose deployment manifest template
/// Placeholders: {deployment_name}, {namespace}, {config_id}, {local_port}
const DEFAULT_EXPOSE_DEPLOYMENT: &str = r#"{
  "apiVersion": "apps/v1",
  "kind": "Deployment",
  "metadata": {
    "name": "{deployment_name}",
    "namespace": "{namespace}",
    "labels": {
      "app": "kftray-expose",
      "config_id": "{config_id}"
    }
  },
  "spec": {
    "replicas": 1,
    "selector": {
      "matchLabels": {
        "app": "kftray-expose",
        "config_id": "{config_id}"
      }
    },
    "template": {
      "metadata": {
        "labels": {
          "app": "kftray-expose",
          "config_id": "{config_id}"
        }
      },
      "spec": {
        "containers": [{
          "name": "kftray-server",
          "image": "ghcr.io/hcavarsan/kftray-server:latest",
          "env": [
            {"name": "PROXY_TYPE", "value": "reverse_http"},
            {"name": "HTTP_PORT", "value": "8080"},
            {"name": "WEBSOCKET_PORT", "value": "9999"},
            {"name": "REMOTE_ADDRESS", "value": "localhost"},
            {"name": "REMOTE_PORT", "value": "{local_port}"},
            {"name": "LOCAL_PORT", "value": "{local_port}"},
            {"name": "RUST_LOG", "value": "DEBUG"}
          ],
          "ports": [
            {"containerPort": 8080, "name": "http"},
            {"containerPort": 9999, "name": "websocket"}
          ]
        }]
      }
    }
  }
}"#;

/// Default expose service manifest template
/// Placeholders: {service_name}, {namespace}, {config_id}, {local_port}
const DEFAULT_EXPOSE_SERVICE: &str = r#"{
  "apiVersion": "v1",
  "kind": "Service",
  "metadata": {
    "name": "{service_name}",
    "namespace": "{namespace}",
    "labels": {
      "app": "kftray-expose",
      "config_id": "{config_id}"
    }
  },
  "spec": {
    "type": "ClusterIP",
    "selector": {
      "app": "kftray-expose",
      "config_id": "{config_id}"
    },
    "ports": [
      {
        "name": "http",
        "port": {local_port},
        "targetPort": 8080,
        "protocol": "TCP"
      },
      {
        "name": "websocket",
        "port": 9999,
        "targetPort": 9999,
        "protocol": "TCP"
      }
    ]
  }
}"#;

/// Default expose ingress manifest template
/// Placeholders: {ingress_name}, {namespace}, {config_id}, {annotations},
/// {ingress_class_name}, {tls}, {domain}, {service_name}, {local_port}
const DEFAULT_EXPOSE_INGRESS: &str = r#"{
  "apiVersion": "networking.k8s.io/v1",
  "kind": "Ingress",
  "metadata": {
    "name": "{ingress_name}",
    "namespace": "{namespace}",
    "labels": {
      "app": "kftray-expose",
      "config_id": "{config_id}"
    }{annotations}
  },
  "spec": {
    {ingress_class_name}
    {tls}
    "rules": [{
      "host": "{domain}",
      "http": {
        "paths": [{
          "path": "/",
          "pathType": "Prefix",
          "backend": {
            "service": {
              "name": "{service_name}",
              "port": {"number": {local_port}}
            }
          }
        }]
      }
    }]
  }
}"#;

fn manifest_file_exists(path: &Path) -> bool {
    path.exists()
}

pub fn create_proxy_deployment_manifest() -> Result<(), Box<dyn std::error::Error>> {
    create_config_dir()?;
    let manifest_path = get_proxy_deployment_manifest_path()?;

    let mut file = File::create(manifest_path)?;
    file.write_all(DEFAULT_PROXY_DEPLOYMENT.as_bytes())?;

    Ok(())
}

pub fn create_expose_deployment_manifest() -> Result<(), Box<dyn std::error::Error>> {
    create_config_dir()?;
    let manifest_path = get_expose_deployment_manifest_path()?;

    let mut file = File::create(manifest_path)?;
    file.write_all(DEFAULT_EXPOSE_DEPLOYMENT.as_bytes())?;

    Ok(())
}

pub fn create_expose_service_manifest() -> Result<(), Box<dyn std::error::Error>> {
    create_config_dir()?;
    let manifest_path = get_expose_service_manifest_path()?;

    let mut file = File::create(manifest_path)?;
    file.write_all(DEFAULT_EXPOSE_SERVICE.as_bytes())?;

    Ok(())
}

pub fn create_expose_ingress_manifest() -> Result<(), Box<dyn std::error::Error>> {
    create_config_dir()?;
    let manifest_path = get_expose_ingress_manifest_path()?;

    let mut file = File::create(manifest_path)?;
    file.write_all(DEFAULT_EXPOSE_INGRESS.as_bytes())?;

    Ok(())
}

pub fn proxy_deployment_manifest_exists() -> bool {
    match get_proxy_deployment_manifest_path() {
        Ok(path) => manifest_file_exists(&path),
        Err(_) => false,
    }
}

pub fn expose_deployment_manifest_exists() -> bool {
    match get_expose_deployment_manifest_path() {
        Ok(path) => manifest_file_exists(&path),
        Err(_) => false,
    }
}

pub fn expose_service_manifest_exists() -> bool {
    match get_expose_service_manifest_path() {
        Ok(path) => manifest_file_exists(&path),
        Err(_) => false,
    }
}

pub fn expose_ingress_manifest_exists() -> bool {
    match get_expose_ingress_manifest_path() {
        Ok(path) => manifest_file_exists(&path),
        Err(_) => false,
    }
}
