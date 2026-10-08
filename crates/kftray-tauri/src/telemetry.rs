use std::any::Any;
use std::panic::{
    Location,
    PanicHookInfo,
};
use std::sync::Arc;
use std::sync::atomic::{
    AtomicBool,
    Ordering,
};

use kftray_commons::utils::settings::get_telemetry_enabled;
use log::warn;
use sentry::integrations::backtrace::current_stacktrace;
use sentry::protocol::{
    Event,
    Exception,
    Level,
    Mechanism,
    Value,
};
use sentry::types::Dsn;
use sentry::{
    ClientInitGuard,
    ClientOptions,
};

const DSN: &str = "https://203f8b8ffea047f9a8bac854d93e6a46@glitchtip.cavarsa.app/1";
const RELEASE: &str = concat!("kftray@", env!("CARGO_PKG_VERSION"));

static ENABLED: AtomicBool = AtomicBool::new(false);

pub fn init() -> ClientInitGuard {
    let mut options = ClientOptions::new();
    options.dsn = dsn();
    options.release = Some(RELEASE.into());
    options.before_send = Some(Arc::new(|event| {
        scrub(ENABLED.load(Ordering::Relaxed), event)
    }));
    let guard = sentry::init(options);

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

pub async fn load_setting() {
    match get_telemetry_enabled().await {
        Ok(enabled) => set_enabled(enabled == Some(true)),
        Err(e) => warn!("Failed to read telemetry setting: {e}"),
    }
}

pub fn capture_frontend_error(name: String, message: String, stack: Option<String>) {
    sentry::capture_event(frontend_event(name, message, stack));
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

fn frontend_event(name: String, message: String, stack: Option<String>) -> Event<'static> {
    let mut event = Event {
        level: Level::Error,
        exception: vec![Exception {
            ty: name,
            value: Some(message),
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
    fn frontend_event_carries_type_message_and_stack() {
        let event = frontend_event(
            "TypeError".into(),
            "x is undefined".into(),
            Some("at main.js:1".into()),
        );
        let exception = &event.exception.values[0];

        assert_eq!(exception.ty, "TypeError");
        assert_eq!(exception.value.as_deref(), Some("x is undefined"));
        assert_eq!(event.extra["stack"], Value::String("at main.js:1".into()));
    }
}
