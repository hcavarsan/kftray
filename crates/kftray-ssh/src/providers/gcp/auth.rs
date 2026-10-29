use std::error::Error;
use std::path::PathBuf;

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
    creds: AccessTokenCredentials,
    adc_present: bool,
}

impl GcpAuth {
    pub(super) fn from_env() -> Result<Self, GcpError> {
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
        Ok(Self {
            creds,
            adc_present: adc_present(),
        })
    }

    async fn token(&self) -> Result<SecretString, GcpError> {
        let token = match timeout(REST_TIMEOUT, self.creds.access_token()).await {
            Ok(Ok(token)) => token,
            Ok(Err(e)) => return Err(self.classify(&e, e.is_transient())),
            Err(_) => {
                return Err(GcpError::Auth {
                    message: "timed out getting a Google access token".into(),
                    retryable: true,
                });
            }
        };
        Ok(SecretString::from(token.token))
    }

    fn classify(&self, error: &(dyn Error + 'static), transient: bool) -> GcpError {
        // Without an ADC file the crate falls back to the metadata server; a
        // failure there only means this machine has no credentials.
        if !self.adc_present {
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
            retryable: transient,
        }
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
