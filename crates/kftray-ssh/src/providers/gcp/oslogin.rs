use std::collections::HashMap;
use std::sync::Arc;
use std::time::{
    SystemTime,
    UNIX_EPOCH,
};

use async_trait::async_trait;
use log::debug;
use russh::keys::Certificate;
use serde::Deserialize;
use serde_json::{
    Value,
    json,
};

use super::compute::{
    Instance,
    region,
};
use super::consts::KEY_TTL;
use super::error::GcpError;
use super::http::{
    query,
    segment,
};
use super::{
    GcpContext,
    GcpInstance,
};
use crate::error::SshError;
use crate::key::KftrayKey;
use crate::route::{
    Authenticator,
    Credentials,
};

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct LoginProfile {
    #[serde(default)]
    posix_accounts: Vec<PosixAccount>,
    #[serde(default)]
    ssh_public_keys: HashMap<String, SshPublicKey>,
}

#[derive(Deserialize)]
struct PosixAccount {
    #[serde(default)]
    primary: bool,
    #[serde(default)]
    username: String,
}

#[derive(Deserialize)]
struct SshPublicKey {
    #[serde(default)]
    key: String,
    #[serde(default)]
    fingerprint: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ImportResponse {
    #[serde(default)]
    login_profile: LoginProfile,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SignResponse {
    signed_ssh_public_key: String,
}

/// Logs in with OS Login: either an imported public key (standard flow) or a
/// short-lived certificate (VMs with `enable-oslogin-certificates`).
pub(super) struct OsLoginAuth {
    ctx: Arc<GcpContext>,
    target: GcpInstance,
    email: String,
}

impl OsLoginAuth {
    pub(super) fn new(ctx: Arc<GcpContext>, target: GcpInstance, email: String) -> Self {
        Self { ctx, target, email }
    }

    async fn login(&self, key: &KftrayKey) -> Result<Credentials, GcpError> {
        // Re-validated on every (re)connection: the VM may have been stopped
        // or switched to another login mode since the route was built.
        let (instance, flags) = self.ctx.inspect(&self.target).await?;
        let public = key.public_openssh();
        if flags.certificates {
            self.certificate(&instance, &public).await
        } else {
            self.standard(key, &public).await
        }
    }

    async fn standard(&self, key: &KftrayKey, public: &str) -> Result<Credentials, GcpError> {
        let profile = self.profile().await?;
        let existing = profile
            .ssh_public_keys
            .iter()
            .find(|(_, entry)| same_key(&entry.key, public))
            .map(|(id, entry)| {
                if entry.fingerprint.is_empty() {
                    id.clone()
                } else {
                    entry.fingerprint.clone()
                }
            });
        let expiration = expiration_usec();
        let profile = match existing {
            Some(fingerprint) if !profile.posix_accounts.is_empty() => {
                let url = format!(
                    "{}/users/{}/sshPublicKeys/{}?updateMask=expirationTimeUsec",
                    self.ctx.endpoints.oslogin,
                    segment(&self.email),
                    segment(&fingerprint)
                );
                let _: Value = self
                    .ctx
                    .http
                    .patch_json(&url, &json!({ "expirationTimeUsec": expiration }))
                    .await
                    .map_err(forbidden)?;
                debug!(
                    "[gcp {}] renewed OS Login key {}",
                    self.target.instance,
                    key.fingerprint()
                );
                profile
            }
            _ => {
                let url = format!(
                    "{}/users/{}:importSshPublicKey?projectId={}&regions={}",
                    self.ctx.endpoints.oslogin,
                    segment(&self.email),
                    query(&self.target.project),
                    query(region(&self.target.zone))
                );
                let imported: ImportResponse = self
                    .ctx
                    .http
                    .post_json(
                        &url,
                        &json!({ "key": public, "expirationTimeUsec": expiration }),
                    )
                    .await
                    .map_err(forbidden)?;
                debug!(
                    "[gcp {}] imported OS Login key {}",
                    self.target.instance,
                    key.fingerprint()
                );
                imported.login_profile
            }
        };
        Ok(Credentials::PublicKey {
            user: self.username(&profile)?,
        })
    }

    async fn certificate(
        &self, instance: &Instance, public: &str,
    ) -> Result<Credentials, GcpError> {
        let project = &self.target.project;
        let region = region(&self.target.zone);
        let sign_url = format!(
            "{}/projects/{}/locations/{}:signSshPublicKey",
            self.ctx.endpoints.oslogin,
            segment(project),
            segment(region)
        );
        let body = json!({
            "sshPublicKey": public,
            "serviceAccount": instance.service_account.as_deref().unwrap_or_default(),
            "computeInstance": format!(
                "projects/{project}/zones/{}/instances/{}",
                self.target.zone, instance.id
            ),
        });
        let signed: Result<SignResponse, GcpError> =
            self.ctx.http.post_json(&sign_url, &body).await;
        let signed = match signed {
            // The account does not exist yet: create it, then sign once more.
            Err(GcpError::Api { status: 404, .. }) => {
                let url = format!(
                    "{}/users/{}/projects/{}",
                    self.ctx.endpoints.oslogin,
                    segment(&self.email),
                    segment(project)
                );
                let _: Value = self
                    .ctx
                    .http
                    .post_json(&url, &json!({ "regions": [region] }))
                    .await
                    .map_err(forbidden)?;
                self.ctx.http.post_json(&sign_url, &body).await
            }
            other => other,
        }
        .map_err(forbidden)?;
        // Kept in memory only.
        let cert = Certificate::from_openssh(&signed.signed_ssh_public_key).map_err(|e| {
            GcpError::Auth {
                message: format!("OS Login returned an invalid certificate: {e}"),
                retryable: false,
            }
        })?;
        debug!("[gcp {}] OS Login certificate signed", self.target.instance);
        let user = self.username(&self.profile().await?)?;
        Ok(Credentials::Certificate { user, cert })
    }

    async fn profile(&self) -> Result<LoginProfile, GcpError> {
        let url = format!(
            "{}/users/{}/loginProfile?projectId={}",
            self.ctx.endpoints.oslogin,
            segment(&self.email),
            query(&self.target.project)
        );
        self.ctx.http.get_json(&url).await
    }

    fn username(&self, profile: &LoginProfile) -> Result<String, GcpError> {
        profile
            .posix_accounts
            .iter()
            .find(|account| account.primary)
            .or_else(|| profile.posix_accounts.first())
            .map(|account| account.username.clone())
            .ok_or_else(|| GcpError::NoPosixAccount {
                email: self.email.clone(),
                project: self.target.project.clone(),
            })
    }
}

#[async_trait]
impl Authenticator for OsLoginAuth {
    async fn credentials(&self, key: &KftrayKey) -> Result<Credentials, SshError> {
        self.login(key).await.map_err(SshError::from)
    }
}

/// Type and base64 only: the comment is not part of the key.
fn same_key(a: &str, b: &str) -> bool {
    a.split_whitespace()
        .take(2)
        .eq(b.split_whitespace().take(2))
}

/// `expirationTimeUsec` is an int64, which the JSON mapping spells as a string.
fn expiration_usec() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    (now + KEY_TTL).as_micros().to_string()
}

fn forbidden(error: GcpError) -> GcpError {
    match error {
        GcpError::Api { status: 403, .. } => GcpError::OsLoginPermission,
        error => error,
    }
}

#[cfg(test)]
mod tests {
    use russh::keys::{
        Algorithm,
        PrivateKey,
    };
    use serde_json::json;
    use tempfile::TempDir;
    use wiremock::matchers::{
        body_json,
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

    use super::*;
    use crate::providers::gcp::test_support::{
        context,
        mount_vm,
        project_json,
        target,
        vm_json,
    };
    use crate::testing::gcp::sign_user_cert;

    const EMAIL: &str = "dev@x.com";

    struct Fixture {
        server: MockServer,
        key: KftrayKey,
        _dir: TempDir,
    }

    impl Fixture {
        async fn new(vm_flags: &[(&str, &str)]) -> Self {
            let server = MockServer::start().await;
            mount_vm(&server, vm_json("RUNNING", vm_flags), project_json(&[])).await;
            let dir = TempDir::new().unwrap();
            Self {
                server,
                key: KftrayKey::load_or_create(dir.path()).unwrap(),
                _dir: dir,
            }
        }

        fn auth(&self, email: &str) -> OsLoginAuth {
            OsLoginAuth::new(context(&self.server), target(), email.into())
        }

        async fn mount_profile(&self, email: &str, profile: Value) {
            Mock::given(method("GET"))
                .and(path(format!("/oslogin/v1/users/{email}/loginProfile")))
                .and(query_param("projectId", "proj"))
                .respond_with(ResponseTemplate::new(200).set_body_json(profile))
                .mount(&self.server)
                .await;
        }

        /// Requests that reached OS Login, as `METHOD /path`.
        async fn oslogin_calls(&self) -> Vec<String> {
            self.requests()
                .await
                .iter()
                .filter(|r| r.url.path().starts_with("/oslogin"))
                .map(|r| format!("{} {}", r.method, r.url.path()))
                .collect()
        }

        async fn requests(&self) -> Vec<Request> {
            self.server.received_requests().await.unwrap()
        }
    }

    fn profile(accounts: Value, keys: Value) -> Value {
        json!({"posixAccounts": accounts, "sshPublicKeys": keys})
    }

    fn account(username: &str, primary: bool) -> Value {
        json!({"username": username, "primary": primary})
    }

    fn user_of(credentials: Credentials) -> String {
        match credentials {
            Credentials::PublicKey { user } | Credentials::Certificate { user, .. } => user,
        }
    }

    #[tokio::test]
    async fn login_should_import_the_key_when_the_profile_lacks_it() {
        let fx = Fixture::new(&[("enable-oslogin", "TRUE")]).await;
        fx.mount_profile(EMAIL, profile(json!([]), json!({}))).await;
        Mock::given(method("POST"))
            .and(path("/oslogin/v1/users/dev@x.com:importSshPublicKey"))
            .and(query_param("projectId", "proj"))
            .and(query_param("regions", "us-central1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"loginProfile": profile(json!([account("sa_1234", true)]), json!({}))}),
            ))
            .mount(&fx.server)
            .await;

        let credentials = fx.auth(EMAIL).login(&fx.key).await.unwrap();

        assert_eq!(user_of(credentials), "sa_1234");
        let imports: Vec<_> = fx
            .requests()
            .await
            .into_iter()
            .filter(|r| r.method.as_str() == "POST")
            .collect();
        assert_eq!(imports.len(), 1);
        let body: Value = serde_json::from_slice(&imports[0].body).unwrap();
        assert_eq!(body["key"], fx.key.public_openssh());
        let expiration: u128 = body["expirationTimeUsec"]
            .as_str()
            .unwrap()
            .parse()
            .unwrap();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_micros();
        assert!(expiration > now);
        assert!(
            fx.oslogin_calls()
                .await
                .iter()
                .all(|c| !c.starts_with("PATCH"))
        );
    }

    #[tokio::test]
    async fn login_should_patch_the_entry_when_the_key_is_already_imported() {
        let fx = Fixture::new(&[("enable-oslogin", "TRUE")]).await;
        // The stored comment differs: only type and base64 identify the key.
        let stored = format!(
            "{} laptop",
            fx.key.public_openssh().rsplit_once(' ').unwrap().0
        );
        fx.mount_profile(
            EMAIL,
            profile(
                json!([account("sa_1234", true)]),
                json!({"hexfp": {"key": stored, "fingerprint": "entry-fp"}}),
            ),
        )
        .await;
        Mock::given(method("PATCH"))
            .and(path("/oslogin/v1/users/dev@x.com/sshPublicKeys/entry-fp"))
            .and(query_param("updateMask", "expirationTimeUsec"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"key": stored})))
            .mount(&fx.server)
            .await;

        let credentials = fx.auth(EMAIL).login(&fx.key).await.unwrap();

        assert_eq!(user_of(credentials), "sa_1234");
        let calls = fx.oslogin_calls().await;
        assert_eq!(
            calls,
            [
                "GET /oslogin/v1/users/dev@x.com/loginProfile",
                "PATCH /oslogin/v1/users/dev@x.com/sshPublicKeys/entry-fp"
            ]
        );
        let patch = fx.requests().await.pop().unwrap();
        let body: Value = serde_json::from_slice(&patch.body).unwrap();
        assert!(body["expirationTimeUsec"].is_string());
    }

    #[tokio::test]
    async fn login_should_import_when_the_key_exists_but_has_no_posix_account() {
        let fx = Fixture::new(&[("enable-oslogin", "TRUE")]).await;
        fx.mount_profile(
            EMAIL,
            profile(json!([]), json!({"fp": {"key": fx.key.public_openssh()}})),
        )
        .await;
        Mock::given(method("POST"))
            .and(path("/oslogin/v1/users/dev@x.com:importSshPublicKey"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"loginProfile": profile(json!([account("sa_1", true)]), json!({}))}),
            ))
            .mount(&fx.server)
            .await;

        fx.auth(EMAIL).login(&fx.key).await.unwrap();

        assert!(
            fx.oslogin_calls()
                .await
                .iter()
                .all(|c| !c.starts_with("PATCH"))
        );
    }

    #[tokio::test]
    async fn login_should_use_the_primary_username_when_it_is_not_the_first() {
        let fx = Fixture::new(&[("enable-oslogin", "TRUE")]).await;
        fx.mount_profile(
            EMAIL,
            profile(
                json!([account("first", false), account("primary", true)]),
                json!({"fp": {"key": fx.key.public_openssh(), "fingerprint": "fp"}}),
            ),
        )
        .await;
        Mock::given(method("PATCH"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .mount(&fx.server)
            .await;

        let credentials = fx.auth(EMAIL).login(&fx.key).await.unwrap();

        assert_eq!(user_of(credentials), "primary");
    }

    #[tokio::test]
    async fn login_should_use_the_first_username_when_none_is_primary() {
        let fx = Fixture::new(&[("enable-oslogin", "TRUE")]).await;
        fx.mount_profile(
            EMAIL,
            profile(
                json!([account("first", false), account("second", false)]),
                json!({"fp": {"key": fx.key.public_openssh(), "fingerprint": "fp"}}),
            ),
        )
        .await;
        Mock::given(method("PATCH"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
            .mount(&fx.server)
            .await;

        let credentials = fx.auth(EMAIL).login(&fx.key).await.unwrap();

        assert_eq!(user_of(credentials), "first");
    }

    #[tokio::test]
    async fn login_should_keep_colon_and_at_sign_in_the_path_when_the_email_is_external() {
        let fx = Fixture::new(&[("enable-oslogin", "TRUE")]).await;
        fx.mount_profile("ext:user@x.com", profile(json!([]), json!({})))
            .await;
        Mock::given(method("POST"))
            .and(path("/oslogin/v1/users/ext:user@x.com:importSshPublicKey"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!({"loginProfile": profile(json!([account("ext_user", true)]), json!({}))}),
            ))
            .mount(&fx.server)
            .await;

        let credentials = fx.auth("ext:user@x.com").login(&fx.key).await.unwrap();

        assert_eq!(user_of(credentials), "ext_user");
    }

    #[tokio::test]
    async fn login_should_fail_with_permission_when_import_answers_403() {
        let fx = Fixture::new(&[("enable-oslogin", "TRUE")]).await;
        fx.mount_profile(EMAIL, profile(json!([]), json!({}))).await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(403).set_body_string("denied"))
            .mount(&fx.server)
            .await;

        let error = fx.auth(EMAIL).login(&fx.key).await.err().unwrap();

        assert!(matches!(error, GcpError::OsLoginPermission));
    }

    #[tokio::test]
    async fn login_should_fail_with_no_posix_account_when_the_profile_has_none() {
        let fx = Fixture::new(&[("enable-oslogin", "TRUE")]).await;
        fx.mount_profile(EMAIL, profile(json!([]), json!({}))).await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"loginProfile": {}})))
            .mount(&fx.server)
            .await;

        let error = fx.auth(EMAIL).login(&fx.key).await.err().unwrap();

        assert!(matches!(
            error,
            GcpError::NoPosixAccount { email, project } if email == EMAIL && project == "proj"
        ));
    }

    #[tokio::test]
    async fn login_should_fail_before_touching_oslogin_when_the_vm_is_not_running() {
        let server = MockServer::start().await;
        mount_vm(
            &server,
            vm_json("STOPPING", &[("enable-oslogin", "TRUE")]),
            project_json(&[]),
        )
        .await;
        let dir = TempDir::new().unwrap();
        let key = KftrayKey::load_or_create(dir.path()).unwrap();

        let auth = OsLoginAuth::new(context(&server), target(), EMAIL.into());
        let error = auth.login(&key).await.err().unwrap();

        assert!(matches!(error, GcpError::InstanceNotRunning { .. }));
        let requests = server.received_requests().await.unwrap();
        assert!(
            requests
                .iter()
                .all(|r| !r.url.path().starts_with("/oslogin"))
        );
    }

    async fn mount_sign(fx: &Fixture, ca: PrivateKey, first_status: Option<u16>) {
        let sign_path = "/oslogin/v1/projects/proj/locations/us-central1:signSshPublicKey";
        if let Some(status) = first_status {
            Mock::given(method("POST"))
                .and(path(sign_path))
                .respond_with(ResponseTemplate::new(status))
                .up_to_n_times(1)
                .mount(&fx.server)
                .await;
        }
        let public = fx.key.public();
        Mock::given(method("POST"))
            .and(path(sign_path))
            .and(body_json(json!({
                "sshPublicKey": fx.key.public_openssh(),
                "serviceAccount": "vm-sa@proj.iam.gserviceaccount.com",
                "computeInstance": "projects/proj/zones/us-central1-b/instances/4242",
            })))
            .respond_with(move |_: &Request| {
                let cert = sign_user_cert(&ca, &public, "sa_1234").unwrap();
                ResponseTemplate::new(200)
                    .set_body_json(json!({"signedSshPublicKey": cert.to_openssh().unwrap()}))
            })
            .mount(&fx.server)
            .await;
    }

    #[tokio::test]
    async fn login_should_return_the_signed_certificate_when_the_vm_uses_certificates() {
        let fx = Fixture::new(&[
            ("enable-oslogin", "TRUE"),
            ("enable-oslogin-certificates", "TRUE"),
        ])
        .await;
        fx.mount_profile(EMAIL, profile(json!([account("sa_1234", true)]), json!({})))
            .await;
        let ca = PrivateKey::random(&mut russh::keys::key::safe_rng(), Algorithm::Ed25519).unwrap();
        mount_sign(&fx, ca, None).await;

        let credentials = fx.auth(EMAIL).login(&fx.key).await.unwrap();

        let Credentials::Certificate { user, cert } = credentials else {
            panic!("expected a certificate");
        };
        assert_eq!(user, "sa_1234");
        assert_eq!(cert.public_key(), fx.key.public().key_data());
    }

    #[tokio::test]
    async fn login_should_provision_the_account_and_sign_again_when_signing_answers_404() {
        let fx = Fixture::new(&[
            ("enable-oslogin", "TRUE"),
            ("enable-oslogin-certificates", "TRUE"),
        ])
        .await;
        fx.mount_profile(EMAIL, profile(json!([account("sa_1234", true)]), json!({})))
            .await;
        let ca = PrivateKey::random(&mut russh::keys::key::safe_rng(), Algorithm::Ed25519).unwrap();
        mount_sign(&fx, ca, Some(404)).await;
        Mock::given(method("POST"))
            .and(path("/oslogin/v1/users/dev@x.com/projects/proj"))
            .and(body_json(json!({"regions": ["us-central1"]})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"username": "sa_1234"})))
            .expect(1)
            .mount(&fx.server)
            .await;

        let credentials = fx.auth(EMAIL).login(&fx.key).await.unwrap();

        assert_eq!(user_of(credentials), "sa_1234");
        let sign_calls = fx
            .oslogin_calls()
            .await
            .into_iter()
            .filter(|c| c.ends_with(":signSshPublicKey"))
            .count();
        assert_eq!(sign_calls, 2);
    }

    #[tokio::test]
    async fn login_should_fail_with_permission_when_signing_answers_403() {
        let fx = Fixture::new(&[
            ("enable-oslogin", "TRUE"),
            ("enable-oslogin-certificates", "TRUE"),
        ])
        .await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(403))
            .mount(&fx.server)
            .await;

        let error = fx.auth(EMAIL).login(&fx.key).await.err().unwrap();

        assert!(matches!(error, GcpError::OsLoginPermission));
    }
}
