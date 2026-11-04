use std::fmt::Display;
use std::sync::Arc;

use percent_encoding::{
    AsciiSet,
    NON_ALPHANUMERIC,
    utf8_percent_encode,
};
use reqwest::{
    Client,
    Method,
    RequestBuilder,
};
use secrecy::ExposeSecret;
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::consts::{
    REST_TIMEOUT,
    USER_AGENT,
};
use super::error::GcpError;
use crate::transport::iap::TokenSource;

/// Keeps `:` and `@` so OS Login paths like `users/ext:a@b.com:import…` stay
/// readable and match what Google documents.
const PATH_SEGMENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~')
    .remove(b':')
    .remove(b'@');

const BODY_LIMIT: usize = 512;

pub(super) fn segment(value: &str) -> impl Display + '_ {
    utf8_percent_encode(value, PATH_SEGMENT)
}

pub(super) fn query(value: &str) -> impl Display + '_ {
    utf8_percent_encode(value, NON_ALPHANUMERIC)
}

/// One REST client per `GcpContext`; the bearer token is fetched per call
/// because the credential crate caches and refreshes it.
pub(super) struct Http {
    client: Client,
    tokens: Arc<dyn TokenSource>,
}

impl Http {
    pub(super) fn new(tokens: Arc<dyn TokenSource>) -> Result<Self, GcpError> {
        let client = Client::builder()
            .timeout(REST_TIMEOUT)
            .user_agent(USER_AGENT)
            .build()
            .map_err(|e| GcpError::Http(e.to_string()))?;
        Ok(Self { client, tokens })
    }

    pub(super) async fn get_json<T: DeserializeOwned>(&self, url: &str) -> Result<T, GcpError> {
        Self::finish(self.authorized(Method::GET, url).await?).await
    }

    pub(super) async fn post_json<T: DeserializeOwned>(
        &self, url: &str, body: &Value,
    ) -> Result<T, GcpError> {
        Self::finish(self.authorized(Method::POST, url).await?.json(body)).await
    }

    pub(super) async fn patch_json<T: DeserializeOwned>(
        &self, url: &str, body: &Value,
    ) -> Result<T, GcpError> {
        Self::finish(self.authorized(Method::PATCH, url).await?.json(body)).await
    }

    /// For endpoints that take the credential in the URL (tokeninfo).
    pub(super) async fn get_anonymous<T: DeserializeOwned>(
        &self, url: &str,
    ) -> Result<T, GcpError> {
        Self::finish(self.client.get(url)).await
    }

    async fn authorized(&self, method: Method, url: &str) -> Result<RequestBuilder, GcpError> {
        let token = self
            .tokens
            .access_token()
            .await
            .map_err(|e| GcpError::Token(Box::new(e)))?;
        Ok(self
            .client
            .request(method, url)
            .bearer_auth(token.expose_secret()))
    }

    async fn finish<T: DeserializeOwned>(request: RequestBuilder) -> Result<T, GcpError> {
        // The URL can carry a token (tokeninfo), so it never reaches an error.
        let http = |e: reqwest::Error| GcpError::Http(e.without_url().to_string());
        let response = request.send().await.map_err(http)?;
        let status = response.status();
        if !status.is_success() {
            let url = response
                .url()
                .as_str()
                .split('?')
                .next()
                .unwrap_or_default()
                .to_owned();
            let body = response.text().await.unwrap_or_default();
            return Err(GcpError::Api {
                url,
                status: status.as_u16(),
                body: body.chars().take(BODY_LIMIT).collect(),
            });
        }
        response.json().await.map_err(http)
    }
}
