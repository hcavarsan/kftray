use std::any::Any;
use std::future::Future;
use std::panic::{
    Location,
    PanicHookInfo,
};
use std::sync::Arc;
use std::sync::atomic::{
    AtomicBool,
    Ordering,
};

use kftray_commons::utils::db_mode::DatabaseMode;
use kftray_commons::utils::settings::{
    get_performance_enabled,
    get_performance_enabled_with_mode,
    get_telemetry_enabled,
    get_telemetry_enabled_with_mode,
};
use log::warn;
use sentry::integrations::backtrace::current_stacktrace;
use sentry::protocol::{
    Event,
    Exception,
    Level,
    Mechanism,
    SpanStatus,
    Value,
};
use sentry::types::Dsn;
use sentry::{
    ClientInitGuard,
    ClientOptions,
    TransactionContext,
};

const DSN: &str = "https://203f8b8ffea047f9a8bac854d93e6a46@glitchtip.cavarsa.app/1";

type SettingResult = Result<Option<bool>, Box<dyn std::error::Error + Send + Sync>>;

const ANONYMOUS_HOST: &str = "anonymous";

static ENABLED: AtomicBool = AtomicBool::new(false);
static PERFORMANCE_ENABLED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    StartForward,
    StopForward,
    StopAllForwards,
}

impl Operation {
    fn name(self) -> &'static str {
        match self {
            Self::StartForward => "portforward.start",
            Self::StopForward => "portforward.stop",
            Self::StopAllForwards => "portforward.stop_all",
        }
    }
}

pub fn init(release: &'static str) -> ClientInitGuard {
    let guard = sentry::init(client_options(release));

    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        sentry::capture_event(panic_event(info));
        if let Some(client) = sentry::Hub::current().client() {
            client.flush(None);
        }
        previous(info);
    }));

    guard
}

pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn set_performance_enabled(enabled: bool) {
    PERFORMANCE_ENABLED.store(enabled, Ordering::Relaxed);
}

pub async fn load_setting() {
    apply(&ENABLED, get_telemetry_enabled().await);
    apply(&PERFORMANCE_ENABLED, get_performance_enabled().await);
}

pub async fn load_setting_with_mode(mode: DatabaseMode) {
    apply(&ENABLED, get_telemetry_enabled_with_mode(mode).await);
    apply(
        &PERFORMANCE_ENABLED,
        get_performance_enabled_with_mode(mode).await,
    );
}

pub async fn measure<T, E>(
    operation: Operation, future: impl Future<Output = Result<T, E>>,
) -> Result<T, E> {
    let transaction =
        sentry::start_transaction(TransactionContext::new(operation.name(), operation.name()));
    let result = future.await;
    transaction.set_status(match result {
        Ok(_) => SpanStatus::Ok,
        Err(_) => SpanStatus::InternalError,
    });
    transaction.finish();
    result
}

pub fn capture_error(name: String, stack: Option<String>) {
    sentry::capture_event(error_event(name, stack));
}

fn client_options(release: &'static str) -> ClientOptions {
    let mut options = ClientOptions::new()
        .traces_sampler(|_| sample_rate(PERFORMANCE_ENABLED.load(Ordering::Relaxed)));
    options.dsn = dsn();
    options.release = Some(release.into());
    options.server_name = Some(ANONYMOUS_HOST.into());
    options.before_send = Some(Arc::new(|event| {
        scrub(ENABLED.load(Ordering::Relaxed), event)
    }));
    options
}

fn sample_rate(enabled: bool) -> f32 {
    if enabled { 1.0 } else { 0.0 }
}

fn apply(flag: &AtomicBool, setting: SettingResult) {
    match setting {
        Ok(enabled) => flag.store(enabled == Some(true), Ordering::Relaxed),
        Err(e) => warn!("Failed to read telemetry setting: {e}"),
    }
}

fn dsn() -> Option<Dsn> {
    if cfg!(debug_assertions) || std::env::var_os("DO_NOT_TRACK").is_some() {
        return None;
    }
    DSN.parse().ok()
}

fn scrub(enabled: bool, mut event: Event<'static>) -> Option<Event<'static>> {
    if !enabled {
        return None;
    }
    event.server_name = None;
    Some(event)
}

fn panic_event(info: &PanicHookInfo<'_>) -> Event<'static> {
    Event {
        level: Level::Fatal,
        exception: vec![Exception {
            ty: "panic".into(),
            value: Some(panic_message(info.payload(), info.location())),
            stacktrace: current_stacktrace(),
            mechanism: Some(Mechanism {
                ty: "panic".into(),
                handled: Some(false),
                ..Default::default()
            }),
            ..Default::default()
        }]
        .into(),
        ..Default::default()
    }
}

fn panic_message(payload: &(dyn Any + Send), location: Option<&Location<'_>>) -> String {
    match payload.downcast_ref::<&'static str>() {
        Some(message) => (*message).to_string(),
        None => location
            .map(|location| format!("{}:{}", location.file(), location.line()))
            .unwrap_or_else(|| "panic".to_string()),
    }
}

fn error_event(name: String, stack: Option<String>) -> Event<'static> {
    let mut event = Event {
        level: Level::Error,
        exception: vec![Exception {
            ty: name,
            ..Default::default()
        }]
        .into(),
        ..Default::default()
    };
    if let Some(stack) = stack {
        event.extra.insert("stack".into(), Value::String(stack));
    }
    event
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event_from(server_name: &str) -> Event<'static> {
        Event {
            server_name: Some(server_name.to_string().into()),
            ..Default::default()
        }
    }

    #[test]
    fn scrub_drops_events_without_consent() {
        assert!(scrub(false, event_from("laptop")).is_none());
    }

    #[test]
    fn scrub_removes_the_host_name_with_consent() {
        let event = scrub(true, event_from("laptop")).unwrap();

        assert_eq!(event.server_name, None);
    }

    #[test]
    fn panic_message_keeps_literal_messages() {
        let message = panic_message(&"index out of range", Some(Location::caller()));

        assert_eq!(message, "index out of range");
    }

    #[test]
    fn panic_message_replaces_formatted_payloads_with_the_location() {
        let location = Location::caller();
        let payload = String::from("called `Result::unwrap()` on https://api.example.com");

        let message = panic_message(&payload, Some(location));

        assert_eq!(message, format!("{}:{}", location.file(), location.line()));
    }

    #[test]
    fn sample_rate_drops_transactions_without_consent() {
        assert_eq!(sample_rate(false), 0.0);
    }

    #[test]
    fn sample_rate_keeps_every_transaction_with_consent() {
        assert_eq!(sample_rate(true), 1.0);
    }

    #[test]
    fn measure_sends_the_operation_name_without_the_host_name() {
        let options = client_options("kftray@test").traces_sample_rate(1.0);

        let envelopes = sentry::test::with_captured_envelopes_options(
            || {
                let result: Result<(), ()> =
                    futures::executor::block_on(measure(Operation::StopForward, async { Err(()) }));
                assert!(result.is_err());
            },
            options,
        );
        let transaction = envelopes
            .iter()
            .flat_map(|envelope| envelope.items())
            .find_map(|item| match item {
                sentry::protocol::EnvelopeItem::Transaction(transaction) => Some(transaction),
                _ => None,
            })
            .unwrap();

        assert_eq!(
            (
                transaction.name.as_deref(),
                transaction.server_name.as_deref()
            ),
            (Some("portforward.stop"), Some(ANONYMOUS_HOST))
        );
    }

    #[test]
    fn error_event_carries_type_and_stack() {
        let event = error_event("TypeError".into(), Some("at main.js:1".into()));
        let exception = &event.exception.values[0];

        assert_eq!(exception.ty, "TypeError");
        assert_eq!(exception.value, None);
        assert_eq!(event.extra["stack"], Value::String("at main.js:1".into()));
    }
}
