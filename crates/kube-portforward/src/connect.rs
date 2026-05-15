use std::time::Duration;

use http::{
    Method,
    Request,
    Uri,
    header,
};

use crate::error::Error;
use crate::subprotocol::Subprotocol;

fn name_is_valid(s: &str) -> bool {
    !s.is_empty()
        && s.is_ascii()
        && s.bytes()
            .all(|b| !matches!(b, b'/' | b'?' | b'#') && !b.is_ascii_control())
}

pub(crate) fn build_portforward_request(
    cluster_url: &Uri, namespace: &str, pod: &str, port: u16, capacity_pairs: usize,
) -> Result<Request<Vec<u8>>, Error> {
    if !name_is_valid(namespace) || !name_is_valid(pod) {
        return Err(Error::Configuration(
            "invalid namespace or pod name: contains forbidden character or non-ASCII".into(),
        ));
    }
    if capacity_pairs == 0 {
        return Err(Error::Configuration("capacity_pairs must be > 0".into()));
    }
    let path = format!("/api/v1/namespaces/{namespace}/pods/{pod}/portforward");
    let query = (0..capacity_pairs)
        .map(|_| format!("ports={port}"))
        .collect::<Vec<_>>()
        .join("&");
    let scheme = cluster_url
        .scheme()
        .ok_or_else(|| Error::Configuration("cluster_url is missing scheme".into()))?;
    let authority = cluster_url
        .authority()
        .ok_or_else(|| Error::Configuration("cluster_url is missing authority".into()))?;
    let uri: Uri = format!("{scheme}://{authority}{path}?{query}")
        .parse()
        .map_err(|e: http::uri::InvalidUri| {
            Error::Configuration(format!("invalid port-forward URI: {e}"))
        })?;
    Request::builder()
        .method(Method::GET)
        .uri(uri)
        .header(
            header::SEC_WEBSOCKET_PROTOCOL,
            Subprotocol::offered_header_value(),
        )
        .body(Vec::new())
        .map_err(|e: http::Error| {
            Error::Configuration(format!("failed to build port-forward request: {e}"))
        })
}

pub(crate) struct KeepaliveConfig {
    pub ping_interval: Duration,
    pub watchdog_timeout: Duration,
}
