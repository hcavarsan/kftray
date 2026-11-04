use std::error::Error;
use std::path::PathBuf;
use std::sync::{
    Arc,
    Mutex,
    PoisonError,
};

use async_trait::async_trait;
use google_cloud_auth::credentials::{
    AccessTokenCredentials,
    Builder,
};
use secrecy::SecretString;
use tokio::time::timeout;

use super::consts::{
    REST_TIMEOUT,
    SCOPES,
};
use super::error::GcpError;
use crate::error::SshError;
use crate::transport::iap::TokenSource;

const CREDENTIALS_ENV: &str = "GOOGLE_APPLICATION_CREDENTIALS";

/// Application Default Credentials. Another identity means another
/// `GOOGLE_APPLICATION_CREDENTIALS`; nothing is stored per config.
pub(super) struct GcpAuth {
    loaded: Mutex<Option<Arc<Loaded>>>,
    build: Box<Build>,
}

type Build = dyn Fn() -> Result<Loaded, GcpError> + Send + Sync;

struct Loaded {
    creds: AccessTokenCredentials,
    adc_present: bool,
}

impl GcpAuth {
    pub(super) fn from_env() -> Result<Self, GcpError> {
        Self::new(Box::new(load_env))
    }

    fn new(build: Box<Build>) -> Result<Self, GcpError> {
        let loaded = Arc::new(build()?);
        Ok(Self {
            loaded: Mutex::new(Some(loaded)),
            build,
        })
    }

    fn current(&self) -> Result<Arc<Loaded>, GcpError> {
        let mut slot = self.loaded.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(loaded) = slot.as_ref() {
            return Ok(Arc::clone(loaded));
        }
        let loaded = Arc::new((self.build)()?);
        *slot = Some(Arc::clone(&loaded));
        Ok(loaded)
    }

    fn invalidate(&self, stale: &Arc<Loaded>) {
        let mut slot = self.loaded.lock().unwrap_or_else(PoisonError::into_inner);
        if slot
            .as_ref()
            .is_some_and(|loaded| Arc::ptr_eq(loaded, stale))
        {
            *slot = None;
        }
    }

    async fn token(&self) -> Result<SecretString, GcpError> {
        let loaded = self.current()?;
        match timeout(REST_TIMEOUT, loaded.creds.access_token()).await {
            Ok(Ok(token)) => Ok(SecretString::from(token.token)),
            Ok(Err(e)) => {
                let transient = e.is_transient();
                if !transient {
                    self.invalidate(&loaded);
                }
                Err(classify(loaded.adc_present, &e, transient))
            }
            Err(_) => Err(GcpError::Auth {
                message: "timed out getting a Google access token".into(),
                retryable: true,
            }),
        }
    }
}

fn load_env() -> Result<Loaded, GcpError> {
    // Scopes only matter for service accounts.
    let creds = Builder::default()
        .with_scopes(SCOPES)
        .build_access_token_credentials()
        .map_err(|e| GcpError::Auth {
            message: if env_set(CREDENTIALS_ENV) {
                format!("{CREDENTIALS_ENV}: {e}")
            } else {
                e.to_string()
            },
            retryable: false,
        })?;
    Ok(Loaded {
        creds,
        adc_present: adc_present(),
    })
}

fn classify(adc_present: bool, error: &(dyn Error + 'static), transient: bool) -> GcpError {
    if transient {
        return GcpError::Auth {
            message: error.to_string(),
            retryable: true,
        };
    }
    // Without an ADC file the crate falls back to the metadata server; a
    // permanent failure there only means this machine has no credentials.
    if !adc_present {
        return GcpError::AdcMissing;
    }
    // The grant text is in the source chain, not in the top-level message.
    let mut chain = String::new();
    let mut current = Some(error);
    while let Some(e) = current {
        chain.push_str(&e.to_string());
        chain.push(' ');
        current = e.source();
    }
    if chain.contains("invalid_grant") || chain.contains("invalid_rapt") {
        return GcpError::TokenRefresh;
    }
    GcpError::Auth {
        message: error.to_string(),
        retryable: false,
    }
}

#[async_trait]
impl TokenSource for GcpAuth {
    async fn access_token(&self) -> Result<SecretString, SshError> {
        self.token().await.map_err(SshError::from)
    }
}

fn env_set(name: &str) -> bool {
    std::env::var_os(name).is_some_and(|v| !v.is_empty())
}

/// Mirrors the lookup of `google-cloud-auth`, which ignores `CLOUDSDK_CONFIG`.
fn adc_present() -> bool {
    if env_set(CREDENTIALS_ENV) {
        return true;
    }
    well_known_adc().is_some_and(|path| path.is_file())
}

#[cfg(windows)]
fn well_known_adc() -> Option<PathBuf> {
    std::env::var_os("APPDATA")
        .map(|root| PathBuf::from(root).join("gcloud/application_default_credentials.json"))
}

#[cfg(not(windows))]
fn well_known_adc() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(|root| PathBuf::from(root).join(".config/gcloud/application_default_credentials.json"))
}

/// A fixed token for tests and e2e.
#[cfg(any(test, feature = "testing", feature = "e2e"))]
pub struct StaticToken(pub SecretString);

#[cfg(any(test, feature = "testing", feature = "e2e"))]
#[async_trait]
impl TokenSource for StaticToken {
    async fn access_token(&self) -> Result<SecretString, SshError> {
        Ok(self.0.clone())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{
        AtomicUsize,
        Ordering,
    };

    use google_cloud_auth::credentials::user_account;
    use secrecy::ExposeSecret;
    use serde_json::json;
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
    use crate::ProviderError;

    fn build_from(server: &MockServer, builds: &Arc<AtomicUsize>) -> Box<Build> {
        let token_uri = format!("{}/token", server.uri());
        let builds = Arc::clone(builds);
        Box::new(move || {
            builds.fetch_add(1, Ordering::SeqCst);
            let creds = user_account::Builder::new(json!({
                "type": "authorized_user",
                "client_id": "id",
                "client_secret": "secret",
                "refresh_token": "refresh",
                "token_uri": token_uri,
            }))
            .with_scopes(SCOPES)
            .build_access_token_credentials()
            .map_err(|e| GcpError::Auth {
                message: e.to_string(),
                retryable: false,
            })?;
            Ok(Loaded {
                creds,
                adc_present: true,
            })
        })
    }

    async fn answer(server: &MockServer, response: ResponseTemplate) {
        server.reset().await;
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(response)
            .mount(server)
            .await;
    }

    fn revoked() -> ResponseTemplate {
        ResponseTemplate::new(400).set_body_json(json!({
            "error": "invalid_grant",
            "error_description": "Token has been expired or revoked.",
        }))
    }

    fn granted() -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "fresh-token",
            "token_type": "Bearer",
            "expires_in": 3600,
        }))
    }

    #[tokio::test]
    async fn token_should_reload_the_credentials_when_the_refresh_was_permanently_rejected() {
        crate::providers::gcp::install_crypto_provider();
        let server = MockServer::start().await;
        let builds = Arc::new(AtomicUsize::new(0));
        answer(&server, revoked()).await;
        let auth = GcpAuth::new(build_from(&server, &builds)).unwrap();

        let error = auth.token().await.unwrap_err();
        assert!(matches!(error, GcpError::TokenRefresh), "{error}");
        answer(&server, granted()).await;
        let token = auth.token().await.unwrap();

        assert_eq!(token.expose_secret(), "fresh-token");
        assert_eq!(builds.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn token_should_reload_the_credentials_once_when_concurrent_requests_fail() {
        crate::providers::gcp::install_crypto_provider();
        let server = MockServer::start().await;
        let builds = Arc::new(AtomicUsize::new(0));
        answer(&server, revoked()).await;
        let auth = GcpAuth::new(build_from(&server, &builds)).unwrap();

        let failures = tokio::join!(auth.token(), auth.token(), auth.token());
        assert!(failures.0.is_err() && failures.1.is_err() && failures.2.is_err());
        assert_eq!(builds.load(Ordering::SeqCst), 1);
        answer(&server, granted()).await;
        let tokens = tokio::join!(auth.token(), auth.token(), auth.token());

        assert!(tokens.0.is_ok() && tokens.1.is_ok() && tokens.2.is_ok());
        assert_eq!(builds.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn classify_should_keep_a_transient_error_retryable_when_no_adc_file_exists() {
        let error = classify(false, &std::io::Error::other("503"), true);

        assert!(matches!(
            error,
            GcpError::Auth {
                retryable: true,
                ..
            }
        ));
        assert!(error.is_retryable());
    }

    #[test]
    fn classify_should_report_missing_credentials_when_no_adc_file_exists_and_the_error_is_permanent()
     {
        let error = classify(false, &std::io::Error::other("no metadata"), false);

        assert!(matches!(error, GcpError::AdcMissing));
        assert!(!error.is_retryable());
    }
}
