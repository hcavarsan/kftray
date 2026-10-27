use std::collections::HashMap;
use std::fs::File;
use std::io::Read;

use kftray_commons::utils::config_dir::{
    get_expose_deployment_manifest_path,
    get_expose_ingress_manifest_path,
    get_expose_service_manifest_path,
};

pub fn render_template(template: &str, values: &HashMap<&str, String>) -> String {
    let mut rendered = template.to_string();
    for (key, value) in values {
        rendered = rendered.replace(&format!("{{{}}}", key), value);
    }
    rendered
}

pub fn load_deployment_template() -> Result<String, String> {
    let manifest_path = get_expose_deployment_manifest_path()
        .map_err(|e| format!("Failed to get manifest path: {}", e))?;
    let mut file = File::open(&manifest_path).map_err(|e| {
        format!(
            "Failed to open expose deployment manifest at {:?}: {}",
            manifest_path, e
        )
    })?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .map_err(|e| format!("Failed to read manifest: {}", e))?;
    Ok(contents)
}

pub fn load_service_template() -> Result<String, String> {
    let manifest_path = get_expose_service_manifest_path()
        .map_err(|e| format!("Failed to get manifest path: {}", e))?;
    let mut file = File::open(&manifest_path).map_err(|e| {
        format!(
            "Failed to open expose service manifest at {:?}: {}",
            manifest_path, e
        )
    })?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .map_err(|e| format!("Failed to read manifest: {}", e))?;
    Ok(contents)
}

pub fn load_ingress_template() -> Result<String, String> {
    let manifest_path = get_expose_ingress_manifest_path()
        .map_err(|e| format!("Failed to get manifest path: {}", e))?;
    let mut file = File::open(&manifest_path).map_err(|e| {
        format!(
            "Failed to open expose ingress manifest at {:?}: {}",
            manifest_path, e
        )
    })?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)
        .map_err(|e| format!("Failed to read manifest: {}", e))?;
    Ok(contents)
}

pub fn build_ingress_annotations(
    cert_manager_enabled: bool, cert_issuer: Option<&str>, cert_issuer_kind: Option<&str>,
    additional_annotations: Option<&str>,
) -> String {
    let mut annotations = Vec::new();

    if cert_manager_enabled {
        let issuer = cert_issuer.unwrap_or("letsencrypt-prod");
        let issuer_kind = cert_issuer_kind.unwrap_or("ClusterIssuer");

        let annotation_key = match issuer_kind {
            "Issuer" => "cert-manager.io/issuer",
            _ => "cert-manager.io/cluster-issuer",
        };

        annotations.push(annotation_entry(annotation_key, issuer));
    }

    if let Some(json_str) = additional_annotations
        && !json_str.trim().is_empty()
    {
        // Try to parse as JSON object
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json_str)
            && let Some(obj) = parsed.as_object()
        {
            for (key, value) in obj {
                if let Some(val_str) = value.as_str() {
                    annotations.push(annotation_entry(key, val_str));
                }
            }
        }
    }

    if !annotations.is_empty() {
        format!(
            r#",
    "annotations": {{
      {}
    }}"#,
            annotations.join(",\n      ")
        )
    } else {
        String::new()
    }
}

fn annotation_entry(key: &str, value: &str) -> String {
    format!(
        "{}: {}",
        serde_json::Value::from(key),
        serde_json::Value::from(value)
    )
}

pub fn build_tls_section(cert_manager_enabled: bool, domain: &str, config_id: &str) -> String {
    if !cert_manager_enabled {
        return String::new();
    }

    format!(
        r#""tls": [{{
      "hosts": ["{}"],
      "secretName": "kftray-expose-tls-{}"
    }}],"#,
        domain, config_id
    )
}

pub fn build_ingress_class_name(ingress_class: Option<&str>) -> String {
    if let Some(class) = ingress_class {
        format!(r#""ingressClassName": "{}","#, class)
    } else {
        String::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn annotations_with_quotes_and_newlines_keep_their_values() {
        let snippet = "more_set_headers \"X-Frame-Options: DENY\";";
        let server_snippet = "location /a {\n  return 404;\n}";
        let additional = serde_json::json!({
            "nginx.ingress.kubernetes.io/configuration-snippet": snippet,
            "nginx.ingress.kubernetes.io/server-snippet": server_snippet,
        })
        .to_string();
        let annotations =
            build_ingress_annotations(true, Some("my \"issuer\""), None, Some(&additional));
        let rendered = render_template(
            r#"{"metadata": {"name": "app"{annotations}}}"#,
            &HashMap::from([("annotations", annotations)]),
        );

        let parsed: serde_json::Value = serde_json::from_str(&rendered).unwrap();
        let parsed = &parsed["metadata"]["annotations"];
        assert_eq!(
            parsed["nginx.ingress.kubernetes.io/configuration-snippet"],
            snippet
        );
        assert_eq!(
            parsed["nginx.ingress.kubernetes.io/server-snippet"],
            server_snippet
        );
        assert_eq!(parsed["cert-manager.io/cluster-issuer"], "my \"issuer\"");
    }
}
