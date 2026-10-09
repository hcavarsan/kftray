//! Opt-in crash reporting and performance data shared by kftray and kftui.
//!
//! Everything that leaves the process goes through this crate so the privacy
//! contract in `docs/kftray/USAGE.md` is enforced in one place. Reports carry
//! only fixed strings from the code, the app version, the build target, the
//! OS and a random per-run id. They never carry cluster names, namespaces,
//! service names, aliases, kubeconfig paths, the host name or log lines.

use std::any::Any;
use std::collections::VecDeque;
use std::panic::{
    Location,
    PanicHookInfo,
};
use std::sync::atomic::{
    AtomicBool,
    Ordering,
};
use std::sync::{
    Arc,
    LazyLock,
};
use std::time::Instant;

use kftray_commons::utils::db_mode::DatabaseMode;
use kftray_commons::utils::settings::{
    get_performance_enabled,
    get_performance_enabled_with_mode,
    get_telemetry_enabled,
    get_telemetry_enabled_with_mode,
};
use log::warn;
use parking_lot::Mutex;
use sentry::integrations::backtrace::current_stacktrace;
use sentry::protocol::{
    Breadcrumb,
    Event,
    Exception,
    Level,
    Mechanism,
    SpanStatus,
    Value,
};
use sentry::types::Uuid;
use sentry::{
    ClientInitGuard,
    ClientOptions,
    Hub,
    SentryFutureExt,
    Span,
    TransactionContext,
};

const DSN: &str = "https://203f8b8ffea047f9a8bac854d93e6a46@glitchtip.cavarsa.app/1";

/// Target triple of this build, e.g. `x86_64-unknown-linux-gnu`. Set by
/// `build.rs`; tells gnu from musl builds apart, which the OS context can't.
const TARGET: &str = env!("KFTRAY_TARGET");

type SettingResult = Result<Option<bool>, Box<dyn std::error::Error + Send + Sync>>;

const ANONYMOUS_HOST: &str = "anonymous";

/// Operation breadcrumbs kept for the next error report. Each is one short
/// fixed string, so this is a few KiB at most.
const MAX_BREADCRUMBS: usize = 50;

static ENABLED: AtomicBool = AtomicBool::new(false);
static PERFORMANCE_ENABLED: AtomicBool = AtomicBool::new(false);
/// Forced in `init`, so uptime counts from launch rather than first report.
static STARTED: LazyLock<Instant> = LazyLock::new(Instant::now);
/// Kept here rather than on a sentry scope: scopes are per thread, and a
/// panic on a tokio worker would otherwise see none of the breadcrumbs that
/// `measure` recorded on other workers.
static BREADCRUMBS: Mutex<VecDeque<Breadcrumb>> = Mutex::new(VecDeque::new());

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

/// A step inside an [`Operation`], reported as a child span so a slow start
/// or stop shows which step took the time. Names are fixed strings; a span
/// never carries a description or data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Picking and claiming a loopback address for the forward.
    AllocateAddress,
    /// Writing the alias to the hosts file.
    HostsEntry,
    /// Building the TLS acceptor for an HTTPS alias.
    Tls,
    /// Loading the kubeconfig, building the client and resolving the target pod.
    Connect,
    /// Binding the local listener and spawning the forwarder.
    Listen,
    /// Recording the running state in the database.
    PersistState,
    /// Creating the relay pod and its service in the cluster.
    RelayDeploy,
    /// Waiting for the relay pod to become ready.
    RelayWait,
    /// Deleting the relay pod and its service from the cluster.
    RelayDelete,
    /// Aborting the local forwarder and closing its listener.
    Shutdown,
    /// Releasing the loopback address back to the pool.
    ReleaseAddress,
    /// Removing the alias from the hosts file.
    HostsCleanup,
}

impl Phase {
    fn name(self) -> &'static str {
        match self {
            Self::AllocateAddress => "forward.allocate_address",
            Self::HostsEntry => "forward.hosts_entry",
            Self::Tls => "forward.tls",
            Self::Connect => "kube.connect",
            Self::Listen => "net.listen",
            Self::PersistState => "db.persist_state",
            Self::RelayDeploy => "relay.deploy",
            Self::RelayWait => "relay.wait_ready",
            Self::RelayDelete => "relay.delete",
            Self::Shutdown => "forward.shutdown",
            Self::ReleaseAddress => "forward.release_address",
            Self::HostsCleanup => "forward.hosts_cleanup",
        }
    }
}

/// Child span of the operation currently being measured; finishes on drop.
/// Outside [`measure`] (or with performance data off) it is a no-op.
#[must_use = "the span finishes when the guard is dropped"]
pub struct PhaseGuard(Option<Span>);

impl PhaseGuard {
    /// Marks the phase as failed. Phases that fail usually return early
    /// through `?`, which drops the guard with the default (ok) status;
    /// call this on the paths where the failure is visible.
    pub fn fail(&self) {
        if let Some(span) = &self.0 {
            span.set_status(SpanStatus::InternalError);
        }
    }
}

impl Drop for PhaseGuard {
    fn drop(&mut self) {
        if let Some(span) = self.0.take() {
            span.finish();
        }
    }
}

/// Starts a child span of the operation currently being measured.
pub fn phase(phase: Phase) -> PhaseGuard {
    let parent = Hub::current().configure_scope(|scope| scope.get_span());
    PhaseGuard(parent.map(|parent| parent.start_child(phase.name(), "")))
}

/// `release` is `<app>@<version>`, e.g. `kftray@0.30.0`; the part before `@`
/// becomes the `app` tag so kftray and kftui events can be told apart.
pub fn init(release: &'static str) -> ClientInitGuard {
    LazyLock::force(&STARTED);
    let guard = sentry::init(client_options(release, reporting_allowed()));

    // Every other thread's hub is derived from the main hub, so tags set
    // there are inherited by tokio workers and by `measure`'s bound hubs.
    Hub::main().configure_scope(|scope| {
        scope.set_tag("app", app_name(release));
        scope.set_tag("target", TARGET);
        // Random per launch and never stored: groups the events of one run
        // without identifying the installation.
        scope.set_tag("run_id", Uuid::new_v4().simple());
    });

    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        sentry::capture_event(panic_event(info));
        if let Some(client) = Hub::current().client() {
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

/// Runs `future` as a transaction named after `operation`. Code awaited
/// inside it (not spawned from it) can attach child spans with [`phase`].
/// The outcome is also kept as a breadcrumb for the next error report.
pub async fn measure<T, E>(
    operation: Operation, future: impl Future<Output = Result<T, E>>,
) -> Result<T, E> {
    // A hub of its own so the transaction can be the scope's span for the
    // whole future even as tokio moves it between worker threads.
    let hub = Arc::new(Hub::new_from_top(Hub::current()));
    let transaction =
        hub.start_transaction(TransactionContext::new(operation.name(), operation.name()));
    hub.configure_scope(|scope| scope.set_span(Some(transaction.clone().into())));

    let result = future.bind_hub(hub).await;

    let (status, level) = match result {
        Ok(_) => (SpanStatus::Ok, Level::Info),
        Err(_) => (SpanStatus::InternalError, Level::Error),
    };
    transaction.set_status(status);
    transaction.finish();
    record_breadcrumb(operation, status, level);
    result
}

pub fn capture_error(name: String, stack: Option<String>) {
    sentry::capture_event(error_event(name, stack));
}

/// `allowed` is the process-wide gate (`DO_NOT_TRACK`, debug builds). It is
/// enforced where events and transactions are emitted, not only on the DSN:
/// `sentry::init` fills an empty DSN from `SENTRY_DSN`, so a `None` DSN alone
/// would let the environment re-enable reporting the user turned off.
fn client_options(release: &'static str, allowed: bool) -> ClientOptions {
    let mut options = ClientOptions::new().traces_sampler(move |_| {
        sample_rate(allowed && PERFORMANCE_ENABLED.load(Ordering::Relaxed))
    });
    options.dsn = if allowed { DSN.parse().ok() } else { None };
    options.release = Some(release.into());
    options.server_name = Some(ANONYMOUS_HOST.into());
    // Frames from our own crates are shown first and used for grouping;
    // everything else (tokio, tauri, std) is collapsed as library code. The
    // crates are listed one by one because `in_app_include` is matched
    // before `in_app_exclude`, and the panic hook in this crate sits on
    // every stack: a bare `kftray` prefix would group every panic together.
    options.in_app_include = vec![
        "kftray_tauri",
        "kftui",
        "kftray_portforward",
        "kftray_commons",
        "kftray_http_logs",
        "kftray_helper",
        "kftray_network_monitor",
        "kftray_shortcuts",
        "kftray_mcp",
    ];
    options.in_app_exclude = vec!["kftray_telemetry"];
    options.before_send = Some(Arc::new(move |event| {
        scrub(allowed && ENABLED.load(Ordering::Relaxed), event)
    }));
    options
}

fn app_name(release: &'static str) -> &'static str {
    release.split_once('@').map_or(release, |(app, _)| app)
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

/// Fixed for the life of the process: `DO_NOT_TRACK` is read once at init,
/// so a transaction sampled in is never from a run that opted out.
fn reporting_allowed() -> bool {
    !cfg!(debug_assertions) && std::env::var_os("DO_NOT_TRACK").is_none()
}

fn record_breadcrumb(operation: Operation, status: SpanStatus, level: Level) {
    let crumb = Breadcrumb {
        category: Some("portforward".into()),
        message: Some(format!("{} {status}", operation.name())),
        level,
        ..Default::default()
    };
    let mut crumbs = BREADCRUMBS.lock();
    if crumbs.len() == MAX_BREADCRUMBS {
        crumbs.pop_front();
    }
    crumbs.push_back(crumb);
}

/// Drops the event without consent; otherwise removes the host name and adds
/// the fixed-string context that helps reading the report: build target,
/// seconds since launch and the operations that ran before it.
fn scrub(enabled: bool, mut event: Event<'static>) -> Option<Event<'static>> {
    if !enabled {
        return None;
    }
    event.server_name = None;
    event.dist = Some(TARGET.into());
    event.extra.insert(
        "uptime_seconds".into(),
        Value::from(STARTED.elapsed().as_secs()),
    );
    let crumbs = BREADCRUMBS.lock();
    event.breadcrumbs.values.extend(crumbs.iter().cloned());
    Some(event)
}

fn panic_event(info: &PanicHookInfo<'_>) -> Event<'static> {
    let mut event = Event {
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
    };
    // Thread names are fixed strings ("main", "tokio-runtime-worker"); they
    // tell a UI-thread panic from a runtime one when frames are missing.
    if let Some(thread) = std::thread::current().name() {
        event.tags.insert("thread".into(), thread.to_string());
    }
    event
}

/// Literal panic messages are kept and get the location appended, so the
/// issue title still tells two `unwrap()`s apart when frames are missing.
/// Formatted payloads may carry a cluster address or a resource name and
/// are reduced to the location alone.
fn panic_message(payload: &(dyn Any + Send), location: Option<&Location<'_>>) -> String {
    let location = location.map(|location| format!("{}:{}", location.file(), location.line()));
    match (payload.downcast_ref::<&'static str>(), location) {
        (Some(message), Some(location)) => format!("{message} ({location})"),
        (Some(message), None) => (*message).to_string(),
        (None, Some(location)) => location,
        (None, None) => "panic".to_string(),
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
    use sentry::protocol::EnvelopeItem;

    use super::*;

    fn event_from(server_name: &str) -> Event<'static> {
        Event {
            server_name: Some(server_name.to_string().into()),
            ..Default::default()
        }
    }

    fn captured_transaction(
        envelopes: &[sentry::Envelope],
    ) -> &sentry::protocol::Transaction<'static> {
        envelopes
            .iter()
            .flat_map(|envelope| envelope.items())
            .find_map(|item| match item {
                EnvelopeItem::Transaction(transaction) => Some(transaction),
                _ => None,
            })
            .unwrap()
    }

    #[test]
    fn scrub_drops_events_without_consent() {
        assert!(scrub(false, event_from("laptop")).is_none());
    }

    #[test]
    fn scrub_removes_the_host_name_and_adds_the_build_target() {
        let event = scrub(true, event_from("laptop")).unwrap();

        assert_eq!(event.server_name, None);
        assert_eq!(event.dist.as_deref(), Some(TARGET));
    }

    #[test]
    fn scrub_attaches_recorded_operations_as_breadcrumbs() {
        record_breadcrumb(
            Operation::StartForward,
            SpanStatus::InternalError,
            Level::Error,
        );

        let event = scrub(true, event_from("laptop")).unwrap();
        let crumb = event
            .breadcrumbs
            .values
            .iter()
            .find(|crumb| crumb.message.as_deref() == Some("portforward.start internal_error"))
            .unwrap();

        assert_eq!(crumb.level, Level::Error);
        assert_eq!(crumb.category.as_deref(), Some("portforward"));
    }

    #[test]
    fn breadcrumbs_are_bounded() {
        for _ in 0..(MAX_BREADCRUMBS * 2) {
            record_breadcrumb(Operation::StopForward, SpanStatus::Ok, Level::Info);
        }

        assert!(BREADCRUMBS.lock().len() <= MAX_BREADCRUMBS);
    }

    #[test]
    fn app_name_is_the_release_prefix() {
        assert_eq!(app_name("kftui@0.30.0"), "kftui");
        assert_eq!(app_name("kftray"), "kftray");
    }

    #[test]
    fn panic_message_keeps_literal_messages_with_the_location() {
        let location = Location::caller();

        let message = panic_message(&"index out of range", Some(location));

        assert_eq!(
            message,
            format!(
                "index out of range ({}:{})",
                location.file(),
                location.line()
            )
        );
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
        let options = client_options("kftray@test", true).traces_sample_rate(1.0);

        let envelopes = sentry::test::with_captured_envelopes_options(
            || {
                let result: Result<(), ()> =
                    futures::executor::block_on(measure(Operation::StopForward, async { Err(()) }));
                assert!(result.is_err());
            },
            options,
        );
        let transaction = captured_transaction(&envelopes);

        assert_eq!(
            (
                transaction.name.as_deref(),
                transaction.server_name.as_deref()
            ),
            (Some("portforward.stop"), Some(ANONYMOUS_HOST))
        );
    }

    #[test]
    fn phases_awaited_inside_measure_become_child_spans() {
        let options = client_options("kftray@test", true).traces_sample_rate(1.0);

        let envelopes = sentry::test::with_captured_envelopes_options(
            || {
                let result: Result<(), ()> =
                    futures::executor::block_on(measure(Operation::StartForward, async {
                        let connect = phase(Phase::Connect);
                        futures::future::ready(()).await;
                        drop(connect);
                        let listen = phase(Phase::Listen);
                        listen.fail();
                        Ok(())
                    }));
                assert!(result.is_ok());
            },
            options,
        );
        let transaction = captured_transaction(&envelopes);
        let spans: Vec<_> = transaction
            .spans
            .iter()
            .map(|span| (span.op.as_deref(), span.status))
            .collect();

        assert_eq!(
            spans,
            vec![
                (Some("kube.connect"), None),
                (Some("net.listen"), Some(SpanStatus::InternalError)),
            ]
        );
    }

    #[test]
    fn phase_outside_measure_is_a_no_op() {
        let guard = phase(Phase::Connect);

        assert!(guard.0.is_none());
    }

    /// Runs `emit` with both consent flags on, under a client that has a DSN
    /// (the test transport sets one, like `SENTRY_DSN` would), and returns
    /// what the client let through.
    fn envelopes_with_consent(allowed: bool, emit: impl FnOnce()) -> Vec<sentry::Envelope> {
        ENABLED.store(true, Ordering::Relaxed);
        PERFORMANCE_ENABLED.store(true, Ordering::Relaxed);
        let envelopes = sentry::test::with_captured_envelopes_options(
            emit,
            client_options("kftray@test", allowed),
        );
        ENABLED.store(false, Ordering::Relaxed);
        PERFORMANCE_ENABLED.store(false, Ordering::Relaxed);
        envelopes
    }

    fn emit_one_of_each() {
        let result: Result<(), ()> =
            futures::executor::block_on(measure(Operation::StartForward, async { Ok(()) }));
        assert!(result.is_ok());
        capture_error("TypeError".into(), None);
    }

    #[test]
    fn opted_out_process_sends_nothing_even_with_consent_and_a_dsn() {
        let envelopes = envelopes_with_consent(false, emit_one_of_each);

        assert!(envelopes.is_empty(), "{envelopes:?}");
    }

    #[test]
    fn allowed_process_sends_events_and_transactions_with_consent() {
        let envelopes = envelopes_with_consent(true, emit_one_of_each);
        let mut kinds: Vec<&str> = envelopes
            .iter()
            .flat_map(|envelope| envelope.items())
            .map(|item| match item {
                EnvelopeItem::Event(_) => "event",
                EnvelopeItem::Transaction(_) => "transaction",
                _ => "other",
            })
            .collect();
        kinds.sort_unstable();

        assert_eq!(kinds, ["event", "transaction"]);
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
