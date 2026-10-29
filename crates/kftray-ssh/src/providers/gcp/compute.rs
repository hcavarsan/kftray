use std::collections::HashMap;

use log::warn;
use russh::keys::PublicKey;
use serde::Deserialize;

use super::consts::HOST_KEY_TYPES;
use super::error::GcpError;
use super::http::segment;
use super::{
    GcpContext,
    GcpInstance,
};

pub(super) struct Instance {
    /// Numeric id; OS Login certificates and host key aliases use it.
    pub(super) id: String,
    pub(super) status: String,
    pub(super) service_account: Option<String>,
    pub(super) metadata: HashMap<String, String>,
}

#[derive(Deserialize)]
struct RawInstance {
    id: String,
    status: String,
    #[serde(default, rename = "serviceAccounts")]
    service_accounts: Vec<ServiceAccount>,
    #[serde(default)]
    metadata: Metadata,
}

#[derive(Deserialize)]
struct ServiceAccount {
    email: String,
}

#[derive(Deserialize, Default)]
struct Metadata {
    #[serde(default)]
    items: Vec<MetadataItem>,
}

#[derive(Deserialize)]
struct MetadataItem {
    key: String,
    value: Option<String>,
}

impl Metadata {
    fn into_map(self) -> HashMap<String, String> {
        self.items
            .into_iter()
            .map(|item| (item.key, item.value.unwrap_or_default()))
            .collect()
    }
}

/// Metadata switches that decide how to log in. The instance value wins over
/// the project value, whatever it is.
pub(super) struct OsLoginFlags {
    pub(super) oslogin: bool,
    pub(super) two_factor: bool,
    pub(super) security_key: bool,
    pub(super) certificates: bool,
    pub(super) guest_attributes: bool,
}

impl OsLoginFlags {
    pub(super) fn resolve(instance: &Instance, project: &HashMap<String, String>) -> Self {
        let flag = |key: &str| {
            instance
                .metadata
                .get(key)
                .or_else(|| project.get(key))
                .is_some_and(|value| value.eq_ignore_ascii_case("true"))
        };
        Self {
            oslogin: flag("enable-oslogin"),
            two_factor: flag("enable-oslogin-2fa"),
            security_key: flag("enable-oslogin-sk"),
            certificates: flag("enable-oslogin-certificates"),
            guest_attributes: flag("enable-guest-attributes"),
        }
    }
}

fn check(name: &str, instance: &Instance, flags: &OsLoginFlags) -> Result<(), GcpError> {
    if instance.status != "RUNNING" {
        return Err(GcpError::InstanceNotRunning {
            instance: name.into(),
            status: instance.status.clone(),
        });
    }
    if !flags.oslogin {
        return Err(GcpError::OsLoginRequired);
    }
    if flags.two_factor || flags.security_key {
        return Err(GcpError::OsLoginInteractive);
    }
    Ok(())
}

/// `us-central1-b` -> `us-central1`.
pub(super) fn region(zone: &str) -> &str {
    zone.rsplit_once('-').map_or(zone, |(region, _)| region)
}

impl GcpContext {
    pub(super) async fn get_instance(&self, target: &GcpInstance) -> Result<Instance, GcpError> {
        let url = format!("{}/{}", self.endpoints.compute, instance_path(target));
        match self.http.get_json::<RawInstance>(&url).await {
            Ok(raw) => Ok(Instance {
                id: raw.id,
                status: raw.status,
                service_account: raw.service_accounts.into_iter().next().map(|sa| sa.email),
                metadata: raw.metadata.into_map(),
            }),
            Err(GcpError::Api { status: 404, .. }) => Err(GcpError::InstanceNotFound {
                instance: target.instance.clone(),
                project: target.project.clone(),
                zone: target.zone.clone(),
            }),
            Err(e) => Err(e),
        }
    }

    async fn get_project_metadata(
        &self, project: &str,
    ) -> Result<HashMap<String, String>, GcpError> {
        #[derive(Deserialize)]
        struct Project {
            #[serde(default, rename = "commonInstanceMetadata")]
            metadata: Metadata,
        }

        let url = format!("{}/projects/{}", self.endpoints.compute, segment(project));
        let project: Project = self.http.get_json(&url).await?;
        Ok(project.metadata.into_map())
    }

    /// Fails fast when the VM cannot be logged in to, so a bad target is
    /// reported before any tunnel is opened.
    pub(super) async fn inspect(
        &self, target: &GcpInstance,
    ) -> Result<(Instance, OsLoginFlags), GcpError> {
        let (instance, project) = tokio::try_join!(
            self.get_instance(target),
            self.get_project_metadata(&target.project)
        )?;
        let flags = OsLoginFlags::resolve(&instance, &project);
        check(&target.instance, &instance, &flags)?;
        Ok((instance, flags))
    }

    /// Host keys the guest agent published; empty when the VM publishes none.
    pub(super) async fn guest_host_keys(
        &self, target: &GcpInstance,
    ) -> Result<Vec<PublicKey>, GcpError> {
        #[derive(Deserialize)]
        struct Response {
            #[serde(default, rename = "queryValue")]
            query_value: QueryValue,
        }
        #[derive(Deserialize, Default)]
        struct QueryValue {
            #[serde(default)]
            items: Vec<Attribute>,
        }
        #[derive(Deserialize)]
        struct Attribute {
            #[serde(default)]
            namespace: String,
            key: String,
            #[serde(default)]
            value: String,
        }

        let url = format!(
            "{}/{}/getGuestAttributes?queryPath=hostkeys%2F",
            self.endpoints.compute,
            instance_path(target)
        );
        let response: Response = match self.http.get_json(&url).await {
            Ok(response) => response,
            Err(GcpError::Api { status: 404, .. }) => return Ok(Vec::new()),
            Err(e) => return Err(e),
        };
        let mut keys = Vec::new();
        for attribute in response.query_value.items {
            if attribute.namespace != "hostkeys"
                || !HOST_KEY_TYPES.contains(&attribute.key.as_str())
            {
                continue;
            }
            // The value is the base64 blob, optionally followed by a comment.
            let blob = attribute
                .value
                .split_whitespace()
                .next()
                .unwrap_or_default();
            match PublicKey::from_openssh(&format!("{} {blob}", attribute.key)) {
                Ok(key) => keys.push(key),
                Err(e) => warn!(
                    "[gcp] ignoring unparsable {} guest attribute of {}: {e}",
                    attribute.key, target.instance
                ),
            }
        }
        Ok(keys)
    }
}

fn instance_path(target: &GcpInstance) -> String {
    format!(
        "projects/{}/zones/{}/instances/{}",
        segment(&target.project),
        segment(&target.zone),
        segment(&target.instance)
    )
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use wiremock::matchers::{
        method,
        path,
        query_param,
    };
    use wiremock::{
        Mock,
        MockServer,
        ResponseTemplate,
    };

    use super::*;
    use crate::providers::gcp::test_support::{
        context,
        mount_vm,
        project_json,
        target,
        vm_json,
    };

    #[tokio::test]
    async fn inspect_should_fail_with_not_running_when_the_vm_is_terminated() {
        let server = MockServer::start().await;
        mount_vm(
            &server,
            vm_json("TERMINATED", &[("enable-oslogin", "TRUE")]),
            project_json(&[]),
        )
        .await;

        let error = context(&server).inspect(&target()).await.err().unwrap();

        assert!(matches!(
            &error,
            GcpError::InstanceNotRunning { instance, status }
                if instance == "jumper" && status == "TERMINATED"
        ));
        assert!(crate::ProviderError::is_retryable(&error));
    }

    #[tokio::test]
    async fn inspect_should_prefer_the_instance_flag_when_the_project_disagrees() {
        let server = MockServer::start().await;
        mount_vm(
            &server,
            vm_json(
                "RUNNING",
                &[("enable-oslogin", "TRUE"), ("enable-oslogin-2fa", "false")],
            ),
            project_json(&[("enable-oslogin", "false"), ("enable-oslogin-2fa", "TRUE")]),
        )
        .await;

        let (_, flags) = context(&server).inspect(&target()).await.unwrap();

        assert!(flags.oslogin);
        assert!(!flags.two_factor);
    }

    #[tokio::test]
    async fn inspect_should_use_the_project_flag_when_the_instance_has_none() {
        let server = MockServer::start().await;
        mount_vm(
            &server,
            vm_json("RUNNING", &[]),
            project_json(&[("enable-oslogin", "True"), ("enable-oslogin-sk", "TRUE")]),
        )
        .await;

        let error = context(&server).inspect(&target()).await.err().unwrap();

        assert!(matches!(error, GcpError::OsLoginInteractive));
    }

    #[tokio::test]
    async fn inspect_should_fail_with_oslogin_required_when_the_flag_is_off() {
        let server = MockServer::start().await;
        mount_vm(
            &server,
            vm_json("RUNNING", &[("enable-oslogin", "FALSE")]),
            project_json(&[("enable-oslogin", "TRUE")]),
        )
        .await;

        let error = context(&server).inspect(&target()).await.err().unwrap();

        assert!(matches!(error, GcpError::OsLoginRequired));
    }

    #[tokio::test]
    async fn get_instance_should_fail_with_not_found_when_the_api_answers_404() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(404).set_body_string("nope"))
            .mount(&server)
            .await;

        let error = context(&server)
            .get_instance(&target())
            .await
            .err()
            .unwrap();

        assert!(matches!(
            error,
            GcpError::InstanceNotFound { instance, project, zone }
                if instance == "jumper" && project == "proj" && zone == "us-central1-b"
        ));
    }

    #[tokio::test]
    async fn guest_host_keys_should_return_only_known_key_types_when_attributes_exist() {
        let host_key = russh::keys::PrivateKey::random(
            &mut russh::keys::key::safe_rng(),
            russh::keys::Algorithm::Ed25519,
        )
        .unwrap();
        let line = host_key.public_key().to_openssh().unwrap();
        let blob = line.split_whitespace().nth(1).unwrap();
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(
                "/compute/v1/projects/proj/zones/us-central1-b/instances/jumper/getGuestAttributes",
            ))
            .and(query_param("queryPath", "hostkeys/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "queryValue": {"items": [
                    {"namespace": "hostkeys", "key": "ssh-ed25519", "value": format!("{blob} root@vm")},
                    {"namespace": "hostkeys", "key": "ssh-dss", "value": "AAAA"},
                    {"namespace": "other", "key": "ssh-rsa", "value": "AAAA"},
                    {"namespace": "hostkeys", "key": "ssh-rsa", "value": "not-base64!"},
                ]}
            })))
            .mount(&server)
            .await;

        let keys = context(&server).guest_host_keys(&target()).await.unwrap();

        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].key_data(), host_key.public_key().key_data());
    }

    #[tokio::test]
    async fn guest_host_keys_should_be_empty_when_the_api_answers_404() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;

        let keys = context(&server).guest_host_keys(&target()).await.unwrap();

        assert!(keys.is_empty());
    }

    #[test]
    fn region_should_drop_the_zone_suffix_when_zone_is_standard() {
        assert_eq!(region("us-central1-b"), "us-central1");
        assert_eq!(
            region("northamerica-northeast1-a"),
            "northamerica-northeast1"
        );
    }
}
