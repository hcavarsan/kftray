use std::sync::LazyLock;
use std::time::{
    Duration,
    SystemTime,
    UNIX_EPOCH,
};

use reqwest::header::CONTENT_TYPE;
use reqwest::{
    Client,
    StatusCode,
    redirect,
};
use sentry::types::{
    Dsn,
    Uuid,
};
use serde::{
    Deserialize,
    Serialize,
};
use thiserror::Error;
use uuid::fmt::{
    Hyphenated,
    Simple,
};

use super::{
    DSN,
    RELEASE,
    TARGET,
    reporting_allowed,
};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_MESSAGE_CHARS: usize = 4000;
const MAX_EMAIL_CHARS: usize = 254;
const MAX_EMAIL_LOCAL_CHARS: usize = 64;
const APP: &str = "kftray";
const SURFACE: &str = "manual-report";
const GENERAL_MESSAGE: &str = "User-submitted problem";
const INFO_LEVEL: &str = "info";
const ENVELOPE_CONTENT_TYPE: &str = "application/x-sentry-envelope";
const AUTH_HEADER: &str = "X-Sentry-Auth";

static CLIENT: LazyLock<Option<Client>> = LazyLock::new(|| {
    Client::builder()
        .timeout(REQUEST_TIMEOUT)
        .connect_timeout(CONNECT_TIMEOUT)
        .redirect(redirect::Policy::none())
        .build()
        .ok()
});

#[derive(Deserialize)]
pub struct ProblemReport {
    pub report_id: Uuid,
    pub message: String,
    #[serde(default)]
    pub email: Option<String>,
    #[serde(default)]
    pub event_id: Option<Uuid>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProblemReportError {
    #[error("Problem reporting is turned off for this build or environment")]
    Disabled,
    #[error("The report id is not valid")]
    InvalidReportId,
    #[error("The associated event id is not valid")]
    InvalidEventId,
    #[error("The report message is empty")]
    EmptyMessage,
    #[error("The report message is longer than {} characters", MAX_MESSAGE_CHARS)]
    MessageTooLong,
    #[error("The contact email is not valid")]
    InvalidEmail,
    #[error("The report could not be prepared")]
    Encode,
    #[error("The report endpoint is not configured correctly")]
    InvalidEndpoint,
    #[error("The report connection could not be set up")]
    Client,
    #[error("The report server did not answer in time")]
    Timeout,
    #[error("The report server could not be reached")]
    Network,
    #[error("The report server is rate limiting reports, try again later")]
    RateLimited,
    #[error("The report server rejected the report (HTTP {0})")]
    Rejected(u16),
}

pub async fn submit(report: ProblemReport) -> Result<(), ProblemReportError> {
    deliver(reporting_allowed(), DSN, RELEASE.get().copied(), &report).await
}

async fn deliver(
    allowed: bool, dsn: &str, release: Option<&str>, report: &ProblemReport,
) -> Result<(), ProblemReportError> {
    if !allowed {
        return Err(ProblemReportError::Disabled);
    }
    let dsn: Dsn = dsn
        .parse()
        .map_err(|_| ProblemReportError::InvalidEndpoint)?;
    let body = encode(&validate(report)?, release)?;
    let client = CLIENT.as_ref().ok_or(ProblemReportError::Client)?;

    let response = client
        .post(dsn.envelope_api_url())
        .header(AUTH_HEADER, dsn.to_auth(None).to_string())
        .header(CONTENT_TYPE, ENVELOPE_CONTENT_TYPE)
        .body(body)
        .send()
        .await
        .map_err(|error| {
            if error.is_timeout() {
                ProblemReportError::Timeout
            } else {
                ProblemReportError::Network
            }
        })?;

    match response.status() {
        status if status.is_success() => Ok(()),
        StatusCode::TOO_MANY_REQUESTS => Err(ProblemReportError::RateLimited),
        status => Err(ProblemReportError::Rejected(status.as_u16())),
    }
}

struct Validated<'a> {
    report_id: Uuid,
    message: &'a str,
    email: Option<&'a str>,
    event_id: Option<Uuid>,
}

fn validate(report: &ProblemReport) -> Result<Validated<'_>, ProblemReportError> {
    if report.report_id.is_nil() {
        return Err(ProblemReportError::InvalidReportId);
    }
    if report.event_id.is_some_and(|id| id.is_nil()) {
        return Err(ProblemReportError::InvalidEventId);
    }

    let message = report.message.trim();
    if message.is_empty() {
        return Err(ProblemReportError::EmptyMessage);
    }
    if message.chars().count() > MAX_MESSAGE_CHARS {
        return Err(ProblemReportError::MessageTooLong);
    }

    let email = match report.email.as_deref().map(str::trim) {
        None | Some("") => None,
        Some(email) if is_valid_email(email) => Some(email),
        Some(_) => return Err(ProblemReportError::InvalidEmail),
    };

    Ok(Validated {
        report_id: report.report_id,
        message,
        email,
        event_id: report.event_id,
    })
}

fn is_valid_email(email: &str) -> bool {
    if email.chars().count() > MAX_EMAIL_CHARS
        || email.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return false;
    }
    let Some((local, domain)) = email.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && local.chars().count() <= MAX_EMAIL_LOCAL_CHARS
        && !domain.contains('@')
        && domain.contains('.')
        && domain
            .split('.')
            .all(|label| !label.is_empty() && !label.starts_with('-') && !label.ends_with('-'))
}

#[derive(Serialize)]
struct Tags {
    app: &'static str,
    surface: &'static str,
    target: &'static str,
}

const TAGS: Tags = Tags {
    app: APP,
    surface: SURFACE,
    target: TARGET,
};

#[derive(Serialize)]
struct EnvelopeHeader {
    event_id: Simple,
}

#[derive(Serialize)]
struct ItemHeader {
    #[serde(rename = "type")]
    kind: &'static str,
}

#[derive(Serialize)]
struct GeneralEvent<'a> {
    event_id: Simple,
    timestamp: f64,
    level: &'static str,
    message: &'static str,
    fingerprint: (&'static str, Hyphenated),
    #[serde(skip_serializing_if = "Option::is_none")]
    release: Option<&'a str>,
    tags: Tags,
    extra: GeneralExtra<'a>,
}

#[derive(Serialize)]
struct GeneralExtra<'a> {
    description: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    contact_email: Option<&'a str>,
}

#[derive(Serialize)]
struct FeedbackEvent<'a> {
    event_id: Simple,
    timestamp: f64,
    level: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    release: Option<&'a str>,
    tags: Tags,
    contexts: FeedbackContexts<'a>,
}

#[derive(Serialize)]
struct FeedbackContexts<'a> {
    feedback: Feedback<'a>,
}

#[derive(Serialize)]
struct Feedback<'a> {
    message: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    contact_email: Option<&'a str>,
    associated_event_id: Simple,
}

fn timestamp() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

fn encode(report: &Validated<'_>, release: Option<&str>) -> Result<Vec<u8>, ProblemReportError> {
    let capacity = report.message.len() + report.email.map_or(0, str::len) + 512;
    match report.event_id {
        Some(associated) => envelope(
            report.report_id,
            "feedback",
            capacity,
            &FeedbackEvent {
                event_id: report.report_id.simple(),
                timestamp: timestamp(),
                level: INFO_LEVEL,
                release,
                tags: TAGS,
                contexts: FeedbackContexts {
                    feedback: Feedback {
                        message: report.message,
                        contact_email: report.email,
                        associated_event_id: associated.simple(),
                    },
                },
            },
        ),
        None => envelope(
            report.report_id,
            "event",
            capacity,
            &GeneralEvent {
                event_id: report.report_id.simple(),
                timestamp: timestamp(),
                level: INFO_LEVEL,
                message: GENERAL_MESSAGE,
                fingerprint: (SURFACE, report.report_id.hyphenated()),
                release,
                tags: TAGS,
                extra: GeneralExtra {
                    description: report.message,
                    contact_email: report.email,
                },
            },
        ),
    }
}

fn envelope(
    report_id: Uuid, kind: &'static str, capacity: usize, payload: &impl Serialize,
) -> Result<Vec<u8>, ProblemReportError> {
    let encode_error = |_| ProblemReportError::Encode;
    let mut body = Vec::with_capacity(capacity);
    serde_json::to_writer(
        &mut body,
        &EnvelopeHeader {
            event_id: report_id.simple(),
        },
    )
    .map_err(encode_error)?;
    body.push(b'\n');
    serde_json::to_writer(&mut body, &ItemHeader { kind }).map_err(encode_error)?;
    body.push(b'\n');
    serde_json::to_writer(&mut body, payload).map_err(encode_error)?;
    body.push(b'\n');
    Ok(body)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::net::TcpListener as StdTcpListener;

    use serde_json::{
        Value,
        json,
    };
    use tokio::io::{
        AsyncReadExt,
        AsyncWriteExt,
    };
    use tokio::net::{
        TcpListener,
        TcpStream,
    };
    use tokio::sync::mpsc;

    use super::*;

    const RELEASE_UNDER_TEST: &str = "kftray@test";

    struct Captured {
        path: String,
        headers: HashMap<String, String>,
        body: Vec<u8>,
    }

    struct Fixture {
        port: u16,
        dsn: String,
        requests: mpsc::UnboundedReceiver<Captured>,
    }

    struct ParsedEnvelope {
        header: Value,
        item_header: Value,
        payload: Value,
    }

    async fn fixture(status: u16, extra_headers: String, body: &'static str) -> Fixture {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (sender, requests) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let Some(captured) = read_request(&mut stream).await else {
                    continue;
                };
                let _ = sender.send(captured);
                let response = format!(
                    "HTTP/1.1 {status} Fixture\r\nContent-Length: {}\r\nConnection: \
                     close\r\n{extra_headers}\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.shutdown().await;
            }
        });
        Fixture {
            port,
            dsn: format!("http://publickey@127.0.0.1:{port}/1"),
            requests,
        }
    }

    async fn read_request(stream: &mut TcpStream) -> Option<Captured> {
        let mut buffer = Vec::new();
        let mut chunk = [0u8; 4096];
        let head_end = loop {
            if let Some(position) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
                break position;
            }
            let read = stream.read(&mut chunk).await.ok()?;
            if read == 0 {
                return None;
            }
            buffer.extend_from_slice(&chunk[..read]);
        };
        let head = String::from_utf8_lossy(&buffer[..head_end]).into_owned();
        let mut lines = head.lines();
        let path = lines.next()?.split_whitespace().nth(1)?.to_owned();
        let headers: HashMap<String, String> = lines
            .filter_map(|line| line.split_once(':'))
            .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_owned()))
            .collect();
        let length: usize = headers.get("content-length")?.parse().ok()?;
        let body_start = head_end + 4;
        while buffer.len() < body_start + length {
            let read = stream.read(&mut chunk).await.ok()?;
            if read == 0 {
                return None;
            }
            buffer.extend_from_slice(&chunk[..read]);
        }
        Some(Captured {
            path,
            headers,
            body: buffer[body_start..body_start + length].to_vec(),
        })
    }

    fn parse_envelope(body: &[u8]) -> ParsedEnvelope {
        let text = std::str::from_utf8(body).unwrap();
        let mut lines = text.lines();
        let header: Value = serde_json::from_str(lines.next().unwrap()).unwrap();
        let item_header: Value = serde_json::from_str(lines.next().unwrap()).unwrap();
        let payload_line = lines.next().unwrap();
        assert!(lines.next().is_none(), "{text}");
        ParsedEnvelope {
            header,
            item_header,
            payload: serde_json::from_str(payload_line).unwrap(),
        }
    }

    fn report(message: &str) -> ProblemReport {
        ProblemReport {
            report_id: Uuid::new_v4(),
            message: message.to_owned(),
            email: None,
            event_id: None,
        }
    }

    fn keys(value: &Value) -> Vec<&str> {
        value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect()
    }

    async fn send_to(
        fixture: &mut Fixture, report: ProblemReport,
    ) -> (Result<(), ProblemReportError>, Option<Captured>) {
        let result = deliver(true, &fixture.dsn, Some(RELEASE_UNDER_TEST), &report).await;
        (result, fixture.requests.try_recv().ok())
    }

    fn validation_error(report: ProblemReport) -> Option<ProblemReportError> {
        validate(&report).err()
    }

    #[test]
    fn validate_rejects_a_blank_message() {
        assert_eq!(
            validation_error(report(" \n\t ")),
            Some(ProblemReportError::EmptyMessage)
        );
    }

    #[test]
    fn validate_trims_the_message() {
        let padded = report("  it broke \n");
        let validated = validate(&padded).ok().unwrap();

        assert_eq!(validated.message, "it broke");
    }

    #[test]
    fn validate_counts_unicode_characters_not_bytes() {
        let at_limit = "é".repeat(MAX_MESSAGE_CHARS);

        assert!(validate(&report(&at_limit)).is_ok());
    }

    #[test]
    fn validate_rejects_a_message_one_character_over_the_limit() {
        let over_limit = "é".repeat(MAX_MESSAGE_CHARS + 1);

        assert_eq!(
            validation_error(report(&over_limit)),
            Some(ProblemReportError::MessageTooLong)
        );
    }

    #[test]
    fn validate_measures_the_limit_after_trimming() {
        let padded = format!("  {}  ", "a".repeat(MAX_MESSAGE_CHARS));

        assert!(validate(&report(&padded)).is_ok());
    }

    #[test]
    fn validate_rejects_nil_ids() {
        let mut nil_report = report("it broke");
        nil_report.report_id = Uuid::nil();
        let mut nil_event = report("it broke");
        nil_event.event_id = Some(Uuid::nil());

        assert_eq!(
            validation_error(nil_report),
            Some(ProblemReportError::InvalidReportId)
        );
        assert_eq!(
            validation_error(nil_event),
            Some(ProblemReportError::InvalidEventId)
        );
    }

    #[test]
    fn validate_treats_a_blank_email_as_absent() {
        let mut blank = report("it broke");
        blank.email = Some("   ".to_owned());

        assert_eq!(validate(&blank).ok().unwrap().email, None);
    }

    #[test]
    fn validate_trims_a_valid_email() {
        let mut padded = report("it broke");
        padded.email = Some(" user@example.com ".to_owned());

        assert_eq!(
            validate(&padded).ok().unwrap().email,
            Some("user@example.com")
        );
    }

    #[test]
    fn email_validation_rejects_malformed_addresses() {
        for email in [
            "plain",
            "@example.com",
            "user@",
            "user@localhost",
            "user@@example.com",
            "us er@example.com",
            "user@exa mple.com",
            "user@example..com",
            "user@.example.com",
            "user@example.com.",
            "user@-example.com",
            "user@example-.com",
            "user\u{7}@example.com",
        ] {
            assert!(!is_valid_email(email), "{email}");
        }
    }

    #[test]
    fn email_validation_accepts_ordinary_addresses() {
        for email in [
            "user@example.com",
            "first.last+tag@sub.example.co",
            "user@xn--bcher-kva.example",
        ] {
            assert!(is_valid_email(email), "{email}");
        }
    }

    #[test]
    fn email_validation_allows_exactly_254_characters() {
        let email = format!(
            "{}@{}.{}.{}",
            "a".repeat(64),
            "b".repeat(63),
            "c".repeat(63),
            "d".repeat(61)
        );

        assert_eq!(email.chars().count(), MAX_EMAIL_CHARS);
        assert!(is_valid_email(&email));
    }

    #[test]
    fn email_validation_rejects_255_characters() {
        let email = format!(
            "{}@{}.{}.{}",
            "a".repeat(64),
            "b".repeat(63),
            "c".repeat(63),
            "d".repeat(62)
        );

        assert_eq!(email.chars().count(), MAX_EMAIL_CHARS + 1);
        assert!(!is_valid_email(&email));
    }

    #[test]
    fn email_validation_rejects_an_overlong_local_part() {
        let email = format!("{}@example.com", "a".repeat(MAX_EMAIL_LOCAL_CHARS + 1));

        assert!(!is_valid_email(&email));
    }

    #[test]
    fn validation_errors_do_not_echo_the_input() {
        let mut rejected = report("my password is hunter2");
        rejected.email = Some("leak me@example.com".to_owned());

        let error = validation_error(rejected).unwrap();

        assert_eq!(error, ProblemReportError::InvalidEmail);
        assert!(!format!("{error} {error:?}").contains("leak"));
    }

    #[tokio::test]
    async fn disabled_reporting_sends_nothing() {
        let mut fixture = fixture(202, String::new(), "").await;

        let (result, request) = send_to_disabled(&mut fixture).await;

        assert_eq!(result, Err(ProblemReportError::Disabled));
        assert!(request.is_none());
    }

    async fn send_to_disabled(
        fixture: &mut Fixture,
    ) -> (Result<(), ProblemReportError>, Option<Captured>) {
        let result = deliver(
            false,
            &fixture.dsn,
            Some(RELEASE_UNDER_TEST),
            &report("it broke"),
        )
        .await;
        (result, fixture.requests.try_recv().ok())
    }

    #[tokio::test]
    async fn invalid_reports_send_nothing() {
        let mut fixture = fixture(202, String::new(), "").await;

        let (result, request) = send_to(&mut fixture, report("   ")).await;

        assert_eq!(result, Err(ProblemReportError::EmptyMessage));
        assert!(request.is_none());
    }

    #[cfg(debug_assertions)]
    #[tokio::test]
    async fn debug_builds_never_submit() {
        assert_eq!(
            submit(report("it broke")).await,
            Err(ProblemReportError::Disabled)
        );
    }

    #[tokio::test]
    async fn invalid_endpoint_is_reported_without_network() {
        let result = deliver(true, "not a dsn", None, &report("it broke")).await;

        assert_eq!(result, Err(ProblemReportError::InvalidEndpoint));
    }

    #[tokio::test]
    async fn general_report_is_one_info_event_with_only_the_explicit_fields() {
        let mut fixture = fixture(200, String::new(), "{}").await;
        let mut general = report("  it broke on start ");
        general.email = Some("user@example.com".to_owned());
        let report_id = general.report_id;

        let (result, request) = send_to(&mut fixture, general).await;
        let request = request.unwrap();
        let sent = parse_envelope(&request.body);

        assert_eq!(result, Ok(()));
        assert_eq!(request.path, "/api/1/envelope/");
        assert_eq!(
            request.headers["content-type"],
            "application/x-sentry-envelope"
        );
        assert!(request.headers["x-sentry-auth"].contains("sentry_key=publickey"));
        assert_eq!(keys(&sent.header), ["event_id"]);
        assert_eq!(sent.header["event_id"], report_id.simple().to_string());
        assert_eq!(sent.item_header["type"], "event");
        assert_eq!(sent.payload["event_id"], report_id.simple().to_string());
        assert_eq!(sent.payload["message"], "User-submitted problem");
        assert_eq!(sent.payload["level"], "info");
        assert_eq!(
            sent.payload["fingerprint"],
            json!(["manual-report", report_id.to_string()])
        );
        assert_eq!(sent.payload["release"], RELEASE_UNDER_TEST);
        assert_eq!(
            sent.payload["tags"],
            json!({ "app": "kftray", "surface": "manual-report", "target": TARGET })
        );
        assert_eq!(
            sent.payload["extra"],
            json!({ "description": "it broke on start", "contact_email": "user@example.com" })
        );
    }

    #[tokio::test]
    async fn general_report_carries_no_ambient_context() {
        let mut fixture = fixture(200, String::new(), "{}").await;

        let (_, request) = send_to(&mut fixture, report("it broke")).await;
        let sent = parse_envelope(&request.unwrap().body);

        let mut payload_keys = keys(&sent.payload);
        payload_keys.sort_unstable();
        assert_eq!(
            payload_keys,
            [
                "event_id",
                "extra",
                "fingerprint",
                "level",
                "message",
                "release",
                "tags",
                "timestamp"
            ]
        );
        assert_eq!(keys(&sent.payload["extra"]), ["description"]);
    }

    #[tokio::test]
    async fn general_reports_get_distinct_fingerprints_and_stable_ones_on_retry() {
        let mut fixture = fixture(200, String::new(), "{}").await;
        let first = report("it broke");
        let second = report("it broke");
        let retry = ProblemReport {
            report_id: first.report_id,
            message: "it broke again".to_owned(),
            email: None,
            event_id: None,
        };

        let mut fingerprints = Vec::new();
        for next in [first, second, retry] {
            let (_, request) = send_to(&mut fixture, next).await;
            let sent = parse_envelope(&request.unwrap().body);
            fingerprints.push(sent.payload["fingerprint"].clone());
        }

        assert_ne!(fingerprints[0], fingerprints[1]);
        assert_eq!(fingerprints[0], fingerprints[2]);
    }

    #[tokio::test]
    async fn linked_report_is_a_feedback_item_for_the_associated_event() {
        let mut fixture = fixture(200, String::new(), "{}").await;
        let associated = Uuid::new_v4();
        let mut linked = report("it broke on start");
        linked.email = Some("user@example.com".to_owned());
        linked.event_id = Some(associated);
        let report_id = linked.report_id;

        let (result, request) = send_to(&mut fixture, linked).await;
        let request = request.unwrap();
        let sent = parse_envelope(&request.body);

        assert_eq!(result, Ok(()));
        assert_eq!(request.path, "/api/1/envelope/");
        assert_eq!(keys(&sent.header), ["event_id"]);
        assert_eq!(sent.header["event_id"], report_id.simple().to_string());
        assert_eq!(sent.item_header["type"], "feedback");
        assert_eq!(sent.payload["event_id"], report_id.simple().to_string());
        assert_eq!(
            sent.payload["contexts"]["feedback"],
            json!({
                "message": "it broke on start",
                "contact_email": "user@example.com",
                "associated_event_id": associated.simple().to_string(),
            })
        );
        assert_eq!(sent.payload["release"], RELEASE_UNDER_TEST);
        assert_eq!(
            sent.payload["tags"],
            json!({ "app": "kftray", "surface": "manual-report", "target": TARGET })
        );
    }

    #[tokio::test]
    async fn linked_report_without_email_omits_the_contact() {
        let mut fixture = fixture(200, String::new(), "{}").await;
        let mut linked = report("it broke");
        linked.event_id = Some(Uuid::new_v4());

        let (_, request) = send_to(&mut fixture, linked).await;
        let sent = parse_envelope(&request.unwrap().body);

        assert!(
            sent.payload["contexts"]["feedback"]
                .get("contact_email")
                .is_none()
        );
    }

    #[tokio::test]
    async fn linked_report_carries_no_automatic_context() {
        let mut fixture = fixture(200, String::new(), "{}").await;
        let mut linked = report("it broke");
        linked.event_id = Some(Uuid::new_v4());

        let (_, request) = send_to(&mut fixture, linked).await;
        let sent = parse_envelope(&request.unwrap().body);

        let mut payload_keys = keys(&sent.payload);
        payload_keys.sort_unstable();
        assert_eq!(
            payload_keys,
            [
                "contexts",
                "event_id",
                "level",
                "release",
                "tags",
                "timestamp"
            ]
        );
        assert_eq!(keys(&sent.payload["contexts"]), ["feedback"]);
    }

    #[tokio::test]
    async fn rate_limiting_is_reported_without_the_response_body() {
        let mut fixture = fixture(429, "Retry-After: 60\r\n".to_owned(), "server secret").await;

        let (result, _) = send_to(&mut fixture, report("typed secret")).await;
        let error = result.unwrap_err();

        assert_eq!(error, ProblemReportError::RateLimited);
        let text = format!("{error} {error:?}");
        assert!(!text.contains("server secret"), "{text}");
        assert!(!text.contains("typed secret"), "{text}");
    }

    #[tokio::test]
    async fn http_errors_report_the_status_only() {
        for status in [400u16, 401, 403, 413, 500, 503] {
            let mut fixture = fixture(status, String::new(), "server secret").await;

            let (result, _) = send_to(&mut fixture, report("typed secret")).await;
            let error = result.unwrap_err();

            assert_eq!(error, ProblemReportError::Rejected(status));
            let text = format!("{error} {error:?}");
            assert!(!text.contains("server secret"), "{text}");
            assert!(!text.contains("typed secret"), "{text}");
        }
    }

    #[tokio::test]
    async fn redirects_are_not_followed_to_another_origin() {
        let mut elsewhere = fixture(200, String::new(), "{}").await;
        let location = format!(
            "Location: http://127.0.0.1:{}/api/1/envelope/\r\n",
            elsewhere.port
        );
        let mut origin = fixture(307, location, "").await;

        let (result, request) = send_to(&mut origin, report("it broke")).await;

        assert_eq!(result, Err(ProblemReportError::Rejected(307)));
        assert!(request.is_some());
        assert!(elsewhere.requests.try_recv().is_err());
    }

    #[tokio::test]
    async fn an_unreachable_server_is_a_network_error() {
        let port = StdTcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let dsn = format!("http://publickey@127.0.0.1:{port}/1");

        let result = deliver(true, &dsn, None, &report("it broke")).await;

        assert_eq!(result, Err(ProblemReportError::Network));
    }
}
