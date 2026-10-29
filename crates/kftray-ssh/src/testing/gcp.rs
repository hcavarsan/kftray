//! Fake Google APIs (Compute, OS Login, tokeninfo) plus a [`FakeIap`] in
//! front of a real sshd, enough to run `bastion_route` end to end.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{
    SystemTime,
    UNIX_EPOCH,
};

use russh::keys::ssh_key::certificate::{
    Builder,
    CertType,
};
use russh::keys::{
    Certificate,
    PrivateKey,
    PublicKey,
};
use serde_json::{
    Value,
    json,
};
use wiremock::matchers::{
    method,
    path,
    query_param,
};
use wiremock::{
    Mock,
    MockServer,
    Request,
    ResponseTemplate,
};

use super::iap::FakeIap;
use crate::error::SshError;
use crate::providers::gcp::{
    GcpContext,
    GcpEndpoints,
    GcpInstance,
    StaticToken,
};

/// Fingerprint of the entry that holds `authorized_key` in the login profile;
/// the PATCH that renews it goes to `.../sshPublicKeys/fake-fp`.
pub const FAKE_FINGERPRINT: &str = "fake-fp";

pub struct FakeGcpConfig {
    pub project: String,
    pub zone: String,
    pub instance: String,
    /// Numeric id of the VM; host keys are stored as `gcp:compute.{id}`.
    pub instance_id: String,
    pub email: String,
    pub posix_user: String,
    /// OpenSSH public key already present in the OS Login profile.
    pub authorized_key: String,
    /// Where the fake IAP tunnel connects to (the sshd).
    pub ssh_upstream: SocketAddr,
    /// Access token accepted by the fake IAP and returned to the client.
    pub token: String,
    /// Serves `enable-oslogin-certificates=TRUE` and signs with `user_ca`.
    pub oslogin_certificates: bool,
    /// Published as a `hostkeys/` guest attribute (and enables guest attributes).
    pub guest_host_key: Option<PublicKey>,
    pub user_ca: Option<PrivateKey>,
}

pub struct FakeGcp {
    server: MockServer,
    iap: FakeIap,
    endpoints: GcpEndpoints,
    target: GcpInstance,
    token: String,
}

impl FakeGcp {
    pub async fn start(config: FakeGcpConfig) -> std::io::Result<Self> {
        let iap = FakeIap::start(config.ssh_upstream, config.token.clone()).await?;
        let server = MockServer::start().await;
        let uri = server.uri();
        let endpoints = GcpEndpoints {
            compute: format!("{uri}/compute/v1"),
            oslogin: format!("{uri}/oslogin/v1"),
            tokeninfo: format!("{uri}/tokeninfo"),
            iap_ws: iap.endpoint(),
        };
        mount_compute(&server, &config).await;
        mount_oslogin(&server, &config).await;
        Mock::given(method("GET"))
            .and(path("/tokeninfo"))
            .and(query_param("access_token", config.token.as_str()))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"email": config.email})))
            .mount(&server)
            .await;
        Ok(Self {
            server,
            iap,
            endpoints,
            target: GcpInstance {
                project: config.project,
                zone: config.zone,
                instance: config.instance,
                interface: "nic0".into(),
            },
            token: config.token,
        })
    }

    pub fn endpoints(&self) -> GcpEndpoints {
        self.endpoints.clone()
    }

    /// The VM the fake serves.
    pub fn target(&self) -> GcpInstance {
        self.target.clone()
    }

    /// A context wired to this fake, with the accepted token.
    pub fn context(&self) -> Result<Arc<GcpContext>, SshError> {
        let tokens = Arc::new(StaticToken(self.token.as_str().into()));
        GcpContext::new(self.endpoints(), tokens).map(Arc::new)
    }

    pub fn iap(&self) -> &FakeIap {
        &self.iap
    }

    /// Every request the REST fakes received, oldest first.
    pub async fn received_requests(&self) -> Vec<Request> {
        self.server.received_requests().await.unwrap_or_default()
    }
}

/// Signs `user` as a user certificate valid for `principal` (what OS Login
/// returns from `signSshPublicKey`).
pub fn sign_user_cert(
    ca: &PrivateKey, user: &PublicKey, principal: &str,
) -> Result<Certificate, russh::keys::ssh_key::Error> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let mut builder = Builder::new_with_random_nonce(
        &mut russh::keys::key::safe_rng(),
        user.key_data().clone(),
        now.saturating_sub(60),
        now + 3600,
    )?;
    builder
        .serial(1)?
        .cert_type(CertType::User)?
        .key_id("kftray-test")?
        .valid_principal(principal)?;
    builder.sign(ca)
}

async fn mount_compute(server: &MockServer, config: &FakeGcpConfig) {
    let FakeGcpConfig {
        project,
        zone,
        instance,
        ..
    } = config;
    let guest_attributes = config.guest_host_key.is_some();
    let mut items = vec![json!({"key": "enable-oslogin", "value": "TRUE"})];
    if config.oslogin_certificates {
        items.push(json!({"key": "enable-oslogin-certificates", "value": "TRUE"}));
    }
    if guest_attributes {
        items.push(json!({"key": "enable-guest-attributes", "value": "TRUE"}));
    }
    let vm_path = format!("/compute/v1/projects/{project}/zones/{zone}/instances/{instance}");
    Mock::given(method("GET"))
        .and(path(vm_path.as_str()))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "id": config.instance_id,
            "name": instance,
            "status": "RUNNING",
            "serviceAccounts": [{"email": format!("vm@{project}.iam.gserviceaccount.com")}],
            "metadata": {"items": items},
        })))
        .mount(server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/compute/v1/projects/{project}")))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "commonInstanceMetadata": {"items": []},
        })))
        .mount(server)
        .await;
    if let Some(host_key) = &config.guest_host_key {
        let attribute = host_key
            .to_openssh()
            .ok()
            .and_then(|line| {
                let mut parts = line.split_whitespace();
                Some((parts.next()?.to_owned(), parts.next()?.to_owned()))
            })
            .map(|(key, value)| json!({"namespace": "hostkeys", "key": key, "value": value}));
        Mock::given(method("GET"))
            .and(path(format!("{vm_path}/getGuestAttributes")))
            .and(query_param("queryPath", "hostkeys/"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "queryValue": {"items": attribute.into_iter().collect::<Vec<_>>()},
            })))
            .mount(server)
            .await;
    }
}

async fn mount_oslogin(server: &MockServer, config: &FakeGcpConfig) {
    let FakeGcpConfig {
        project,
        zone,
        email,
        ..
    } = config;
    let region = zone
        .rsplit_once('-')
        .map_or(zone.as_str(), |(region, _)| region);
    let user = format!("/oslogin/v1/users/{email}");
    let profile = json!({
        "posixAccounts": [{"primary": true, "username": config.posix_user}],
        "sshPublicKeys": {
            (FAKE_FINGERPRINT): {"key": config.authorized_key, "fingerprint": FAKE_FINGERPRINT},
        },
    });
    let ok = |body: Value| ResponseTemplate::new(200).set_body_json(body);

    Mock::given(method("GET"))
        .and(path(format!("{user}/loginProfile")))
        .respond_with(ok(profile.clone()))
        .mount(server)
        .await;
    Mock::given(method("PATCH"))
        .and(path(format!("{user}/sshPublicKeys/{FAKE_FINGERPRINT}")))
        .respond_with(ok(
            json!({"key": config.authorized_key, "fingerprint": FAKE_FINGERPRINT}),
        ))
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("{user}:importSshPublicKey")))
        .respond_with(ok(json!({"loginProfile": profile})))
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path(format!("{user}/projects/{project}")))
        .respond_with(ok(json!({})))
        .mount(server)
        .await;

    if let Some(ca) = config.user_ca.clone() {
        let principal = config.posix_user.clone();
        Mock::given(method("POST"))
            .and(path(format!(
                "/oslogin/v1/projects/{project}/locations/{region}:signSshPublicKey"
            )))
            .respond_with(move |request: &Request| {
                let signed = serde_json::from_slice::<Value>(&request.body)
                    .ok()
                    .and_then(|body| body["sshPublicKey"].as_str().map(str::to_owned))
                    .and_then(|key| PublicKey::from_openssh(&key).ok())
                    .and_then(|key| sign_user_cert(&ca, &key, &principal).ok())
                    .and_then(|cert| cert.to_openssh().ok());
                match signed {
                    Some(cert) => ResponseTemplate::new(200)
                        .set_body_json(json!({"signedSshPublicKey": cert})),
                    None => ResponseTemplate::new(400),
                }
            })
            .mount(server)
            .await;
    }
}
