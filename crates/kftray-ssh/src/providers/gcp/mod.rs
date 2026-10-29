//! `gcp-bastion`: SSH to a Compute Engine VM through IAP TCP forwarding,
//! logging in with OS Login.

mod auth;
mod compute;
mod consts;
mod endpoints;
mod error;
mod http;
mod oslogin;
mod route;

use std::sync::Arc;

#[cfg(any(test, feature = "testing", feature = "e2e"))]
pub use auth::StaticToken;
pub use endpoints::GcpEndpoints;
pub use error::GcpError;
use http::Http;
pub use route::{
    GcpInstance,
    bastion_route,
};
use secrecy::ExposeSecret;

use crate::error::SshError;
use crate::transport::iap::TokenSource;

/// Credentials, endpoints and the REST client shared by every route.
pub struct GcpContext {
    tokens: Arc<dyn TokenSource>,
    endpoints: GcpEndpoints,
    http: Http,
}

impl GcpContext {
    /// Application Default Credentials and the Google endpoints. With the
    /// `e2e` feature, `KFTRAY_GCP_ENDPOINTS` (JSON `GcpEndpoints`) and
    /// `KFTRAY_GCP_STATIC_TOKEN` replace both.
    pub fn from_env() -> Result<Self, SshError> {
        install_crypto_provider();
        #[cfg(feature = "e2e")]
        if let Ok(raw) = std::env::var(consts::ENV_ENDPOINTS) {
            let auth_error = |message: String| GcpError::Auth {
                message,
                retryable: false,
            };
            let endpoints = serde_json::from_str(&raw)
                .map_err(|e| auth_error(format!("{}: {e}", consts::ENV_ENDPOINTS)))?;
            let token = std::env::var(consts::ENV_STATIC_TOKEN).map_err(|_| {
                auth_error(format!(
                    "{} is required with {}",
                    consts::ENV_STATIC_TOKEN,
                    consts::ENV_ENDPOINTS
                ))
            })?;
            return Ok(Self::build(endpoints, Arc::new(StaticToken(token.into())))?);
        }
        Ok(Self::build(
            GcpEndpoints::default(),
            Arc::new(auth::GcpAuth::from_env()?),
        )?)
    }

    /// Custom endpoints and token source (tests and e2e).
    #[cfg(any(test, feature = "testing", feature = "e2e"))]
    pub fn new(endpoints: GcpEndpoints, tokens: Arc<dyn TokenSource>) -> Result<Self, SshError> {
        install_crypto_provider();
        Ok(Self::build(endpoints, tokens)?)
    }

    fn build(endpoints: GcpEndpoints, tokens: Arc<dyn TokenSource>) -> Result<Self, GcpError> {
        Ok(Self {
            http: Http::new(Arc::clone(&tokens))?,
            tokens,
            endpoints,
        })
    }

    /// E-mail of the credential, used as the OS Login user.
    pub(crate) async fn email(&self) -> Result<String, GcpError> {
        #[derive(serde::Deserialize)]
        struct TokenInfo {
            email: Option<String>,
        }

        let token = self
            .tokens
            .access_token()
            .await
            .map_err(|e| GcpError::Token(Box::new(e)))?;
        let url = format!(
            "{}?access_token={}",
            self.endpoints.tokeninfo,
            http::query(token.expose_secret())
        );
        let info: TokenInfo = self.http.get_anonymous(&url).await?;
        info.email
            .filter(|email| !email.is_empty())
            .ok_or(GcpError::IdentityUnknown)
    }
}

/// `reqwest` and `google-cloud-auth` are built without a TLS provider and
/// panic without a process default.
fn install_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

#[cfg(test)]
mod test_support {
    use serde_json::{
        Value,
        json,
    };
    use wiremock::matchers::{
        method,
        path,
    };
    use wiremock::{
        Mock,
        MockServer,
        ResponseTemplate,
    };

    use super::*;

    pub(super) fn target() -> GcpInstance {
        GcpInstance {
            project: "proj".into(),
            zone: "us-central1-b".into(),
            instance: "jumper".into(),
            interface: "nic0".into(),
        }
    }

    pub(super) fn context(server: &MockServer) -> Arc<GcpContext> {
        let uri = server.uri();
        let endpoints = GcpEndpoints {
            compute: format!("{uri}/compute/v1"),
            oslogin: format!("{uri}/oslogin/v1"),
            tokeninfo: format!("{uri}/tokeninfo"),
            iap_ws: "ws://unused.invalid".into(),
        };
        Arc::new(GcpContext::new(endpoints, Arc::new(StaticToken("test-token".into()))).unwrap())
    }

    fn metadata(items: &[(&str, &str)]) -> Value {
        let items: Vec<_> = items
            .iter()
            .map(|(key, value)| json!({"key": key, "value": value}))
            .collect();
        json!({"items": items})
    }

    pub(super) fn vm_json(status: &str, metadata_items: &[(&str, &str)]) -> Value {
        json!({
            "id": "4242",
            "name": "jumper",
            "status": status,
            "serviceAccounts": [{"email": "vm-sa@proj.iam.gserviceaccount.com"}],
            "metadata": metadata(metadata_items),
        })
    }

    pub(super) fn project_json(items: &[(&str, &str)]) -> Value {
        json!({"commonInstanceMetadata": metadata(items)})
    }

    pub(super) async fn mount_vm(server: &MockServer, vm: Value, project: Value) {
        Mock::given(method("GET"))
            .and(path(
                "/compute/v1/projects/proj/zones/us-central1-b/instances/jumper",
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(vm))
            .mount(server)
            .await;
        Mock::given(method("GET"))
            .and(path("/compute/v1/projects/proj"))
            .respond_with(ResponseTemplate::new(200).set_body_json(project))
            .mount(server)
            .await;
    }
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

    use super::test_support::context;
    use super::*;

    #[tokio::test]
    async fn email_should_return_the_tokeninfo_email_when_the_token_is_introspectable() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/tokeninfo"))
            .and(query_param("access_token", "test-token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"email": "dev@x.com"})))
            .mount(&server)
            .await;

        assert_eq!(context(&server).email().await.unwrap(), "dev@x.com");
    }

    #[tokio::test]
    async fn email_should_fail_with_identity_unknown_when_tokeninfo_has_no_email() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"aud": "x"})))
            .mount(&server)
            .await;

        let error = context(&server).email().await.unwrap_err();

        assert!(matches!(error, GcpError::IdentityUnknown));
    }

    #[tokio::test]
    async fn email_should_report_the_url_without_the_token_when_the_api_fails() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(400).set_body_string("invalid"))
            .mount(&server)
            .await;

        let error = context(&server).email().await.unwrap_err();

        let GcpError::Api { url, status, .. } = &error else {
            panic!("expected an API error, got {error}");
        };
        assert_eq!(*status, 400);
        assert!(!url.contains("test-token") && !url.contains('?'));
    }
}
