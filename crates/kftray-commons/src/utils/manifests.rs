use std::fs::File;
use std::io::Write;
use std::path::Path;

use serde_json::json;

use crate::utils::config_dir::{
    create_config_dir,
    get_expose_deployment_manifest_path,
    get_expose_ingress_manifest_path,
    get_expose_service_manifest_path,
    get_pod_manifest_path,
    get_proxy_deployment_manifest_path,
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
        "terminationGracePeriodSeconds": 10,
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

/// Body of [`DEFAULT_PROXY_DEPLOYMENT`] before `terminationGracePeriodSeconds`
/// was added. Kept so an existing install's unmodified manifest is not
/// reported as customized, and so `db::init` can migrate it forward.
const PREVIOUS_DEFAULT_PROXY_DEPLOYMENT: &str = r#"{
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
        "terminationGracePeriodSeconds": 10,
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

/// Default proxy pod manifest (legacy, Pod-based)
/// Placeholders: {hashed_name}, {config_id}, {local_port}, {remote_port},
/// {remote_address}, {protocol}
pub fn default_pod_manifest() -> serde_json::Value {
    json!({
        "apiVersion": "v1",
        "kind": "Pod",
        "metadata": {
            "name": "{hashed_name}",
            "labels": {
                "app": "{hashed_name}",
                "config_id": "{config_id}"
            }
        },
        "spec": {
            "terminationGracePeriodSeconds": 10,
            "containers": [{
                "name": "{hashed_name}",
                "image": "ghcr.io/hcavarsan/kftray-server:latest",
                "env": [
                    {"name": "LOCAL_PORT", "value": "{local_port}"},
                    {"name": "REMOTE_PORT", "value": "{remote_port}"},
                    {"name": "REMOTE_ADDRESS", "value": "{remote_address}"},
                    {"name": "PROXY_TYPE", "value": "{protocol}"},
                    {"name": "RUST_LOG", "value": "DEBUG"},
                ],
                "resources": {
                    "limits": {
                        "cpu": "100m",
                        "memory": "200Mi"
                    },
                    "requests": {
                        "cpu": "100m",
                        "memory": "100Mi"
                    }
                }
            }],
        }
    })
}

/// Body of [`default_pod_manifest`] before `terminationGracePeriodSeconds` was
/// added. Kept so an existing install's unmodified manifest is not reported
/// as customized, and so `db::init` can migrate it forward.
fn previous_default_pod_manifest() -> serde_json::Value {
    json!({
        "apiVersion": "v1",
        "kind": "Pod",
        "metadata": {
            "name": "{hashed_name}",
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
                    {"name": "RUST_LOG", "value": "DEBUG"},
                ],
                "resources": {
                    "limits": {
                        "cpu": "100m",
                        "memory": "200Mi"
                    },
                    "requests": {
                        "cpu": "100m",
                        "memory": "100Mi"
                    }
                }
            }],
        }
    })
}

/// Reports whether the on-disk Pod manifest differs from
/// [`default_pod_manifest`]. A missing manifest is not customized; an
/// unreadable or malformed one is treated as customized so a user's file is
/// never bypassed.
pub fn pod_manifest_is_customized() -> bool {
    let Ok(path) = get_pod_manifest_path() else {
        return true;
    };
    manifest_is_customized(
        &path,
        &[&default_pod_manifest(), &previous_default_pod_manifest()],
    )
}

/// Reports whether the on-disk Deployment manifest differs from the default.
///
/// The two manifests are edited separately, so a default Pod template says
/// nothing about the Deployment actually being applied.
pub fn deployment_manifest_is_customized() -> bool {
    let Ok(path) = get_proxy_deployment_manifest_path() else {
        return true;
    };
    let Ok(default) = serde_json::from_str::<serde_json::Value>(DEFAULT_PROXY_DEPLOYMENT) else {
        return true;
    };
    let Ok(previous) = serde_json::from_str::<serde_json::Value>(PREVIOUS_DEFAULT_PROXY_DEPLOYMENT)
    else {
        return true;
    };
    manifest_is_customized(&path, &[&default, &previous])
}

/// Rewrites an on-disk manifest that still matches a superseded default body
/// with the current default, so an upgrading install picks up new fields
/// (e.g. `terminationGracePeriodSeconds`) without ever having "customized"
/// the file itself.
pub fn migrate_pod_manifest_if_previous_default() -> std::io::Result<()> {
    let path = get_pod_manifest_path().map_err(std::io::Error::other)?;
    if !manifest_matches(&path, &previous_default_pod_manifest()) {
        return Ok(());
    }
    let manifest_json = serde_json::to_string_pretty(&default_pod_manifest())?;
    std::fs::write(path, manifest_json)
}

pub fn migrate_proxy_deployment_manifest_if_previous_default() -> std::io::Result<()> {
    let path = get_proxy_deployment_manifest_path().map_err(std::io::Error::other)?;
    let Ok(previous) = serde_json::from_str::<serde_json::Value>(PREVIOUS_DEFAULT_PROXY_DEPLOYMENT)
    else {
        return Ok(());
    };
    if !manifest_matches(&path, &previous) {
        return Ok(());
    }
    std::fs::write(path, DEFAULT_PROXY_DEPLOYMENT)
}

fn manifest_matches(path: &std::path::Path, expected: &serde_json::Value) -> bool {
    match std::fs::read_to_string(path) {
        Ok(contents) => matches!(
            serde_json::from_str::<serde_json::Value>(&contents),
            Ok(value) if &value == expected
        ),
        Err(_) => false,
    }
}

fn manifest_is_customized(path: &std::path::Path, defaults: &[&serde_json::Value]) -> bool {
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return false,
        Err(_) => return true,
        Ok(_) => {}
    }
    match std::fs::read_to_string(path) {
        Ok(contents) => match serde_json::from_str::<serde_json::Value>(&contents) {
            Ok(manifest) => !defaults.contains(&&manifest),
            Err(_) => true,
        },
        Err(_) => true,
    }
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

#[cfg(test)]
mod tests {
    use std::env;
    use std::sync::Mutex;

    use lazy_static::lazy_static;
    use tempfile::tempdir;

    use super::*;
    use crate::utils::config_dir::{
        get_pod_manifest_path,
        get_proxy_deployment_manifest_path,
    };

    lazy_static! {
        static ref ENV_TEST_MUTEX: Mutex<()> = Mutex::new(());
    }

    struct StrictEnvGuard {
        saved_vars: Vec<(String, Option<String>)>,
    }

    impl StrictEnvGuard {
        fn new(keys: &[&str]) -> Self {
            let saved_vars = keys
                .iter()
                .map(|&key| (key.to_string(), env::var(key).ok()))
                .collect::<Vec<_>>();

            for key in keys {
                unsafe { env::remove_var(key) };
            }

            StrictEnvGuard { saved_vars }
        }
    }

    impl Drop for StrictEnvGuard {
        fn drop(&mut self) {
            for (key, value) in self.saved_vars.drain(..) {
                match value {
                    Some(val) => unsafe { env::set_var(key, val) },
                    None => unsafe { env::remove_var(key) },
                }
            }
        }
    }

    #[test]
    fn a_manifest_equal_to_the_previous_default_is_not_customized_and_gets_migrated() {
        let _lock = ENV_TEST_MUTEX.lock().unwrap();
        let _guard = StrictEnvGuard::new(&["KFTRAY_CONFIG", "XDG_CONFIG_HOME", "HOME"]);

        let temp_dir = tempdir().unwrap();
        unsafe { env::set_var("KFTRAY_CONFIG", temp_dir.path().to_str().unwrap()) };
        create_config_dir().unwrap();

        let pod_path = get_pod_manifest_path().unwrap();
        std::fs::write(
            &pod_path,
            serde_json::to_string_pretty(&previous_default_pod_manifest()).unwrap(),
        )
        .unwrap();

        let deployment_path = get_proxy_deployment_manifest_path().unwrap();
        std::fs::write(&deployment_path, PREVIOUS_DEFAULT_PROXY_DEPLOYMENT).unwrap();

        assert!(
            !pod_manifest_is_customized(),
            "a Pod manifest equal to the previous default must not be reported as customized"
        );
        assert!(
            !deployment_manifest_is_customized(),
            "a Deployment manifest equal to the previous default must not be reported as \
             customized"
        );

        migrate_pod_manifest_if_previous_default().unwrap();
        migrate_proxy_deployment_manifest_if_previous_default().unwrap();

        let migrated_pod: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&pod_path).unwrap()).unwrap();
        assert_eq!(
            migrated_pod,
            default_pod_manifest(),
            "init's migration must rewrite the Pod manifest to the current default"
        );

        let migrated_deployment: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&deployment_path).unwrap()).unwrap();
        assert_eq!(
            migrated_deployment,
            serde_json::from_str::<serde_json::Value>(DEFAULT_PROXY_DEPLOYMENT).unwrap(),
            "init's migration must rewrite the Deployment manifest to the current default"
        );

        assert!(!pod_manifest_is_customized());
        assert!(!deployment_manifest_is_customized());
    }
}
