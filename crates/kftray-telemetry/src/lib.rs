//! Opt-in crash reporting and performance data shared by kftray and kftui.

pub mod problem_reports;

use std::any::Any;
use std::collections::VecDeque;
use std::fmt::Display;
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
    OnceLock,
};
use std::time::{
    Instant,
    SystemTime,
};

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
    SpanId,
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
    Transaction,
    TransactionContext,
};
use serde::Serialize;

const DSN: &str = "https://203f8b8ffea047f9a8bac854d93e6a46@glitchtip.cavarsa.app/1";

/// Target triple of this build, e.g. `x86_64-unknown-linux-gnu`. Set by
/// `build.rs`; tells gnu from musl builds apart, which the OS context can't.
const TARGET: &str = env!("KFTRAY_TARGET");

type SettingResult = Result<Option<bool>, Box<dyn std::error::Error + Send + Sync>>;

const ANONYMOUS_HOST: &str = "anonymous";

/// Operation breadcrumbs kept for the next error report. Each is one short
/// fixed string, so this is a few KiB at most.
const MAX_BREADCRUMBS: usize = 50;
/// Random per launch and never stored: groups the events of one run (from
/// the Rust process and the webview alike) without identifying the install.
static RUN_ID: LazyLock<String> = LazyLock::new(|| Uuid::new_v4().simple().to_string());
/// Set once by `init`; runtime input, hence `OnceLock`.
static RELEASE: OnceLock<&'static str> = OnceLock::new();

static ENABLED: AtomicBool = AtomicBool::new(false);
static PERFORMANCE_ENABLED: Mutex<bool> = Mutex::new(false);
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
    /// A forward started on launch because it was running when the app
    /// last exited.
    AutoStart,
    /// Database and settings initialisation on launch.
    AppStartup,
    /// Importing configurations from a git repository.
    GitSync,
    /// Starting the MCP server.
    McpStart,
}

impl Operation {
    fn name(self) -> &'static str {
        match self {
            Self::StartForward => "portforward.start",
            Self::StopForward => "portforward.stop",
            Self::StopAllForwards => "portforward.stop_all",
            Self::AutoStart => "portforward.autostart",
            Self::AppStartup => "app.startup",
            Self::GitSync => "git.sync",
            Self::McpStart => "mcp.start",
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
    /// Opening the SQLite database and creating its tables.
    DbInit,
    /// Running the configuration migrations.
    DbMigrate,
    /// Reading the telemetry settings.
    SettingsLoad,
    /// Cloning or fetching the repository.
    GitFetch,
    /// Parsing the configuration files and writing them to the database.
    GitImport,
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
            Self::DbInit => "db.init",
            Self::DbMigrate => "db.migrate",
            Self::SettingsLoad => "settings.load",
            Self::GitFetch => "git.fetch",
            Self::GitImport => "git.import",
        }
    }
}

/// Why an operation failed, reduced to a fixed vocabulary so a report can
/// say "timeout" or "kubeconfig" without carrying the message, which names
/// clusters, namespaces and services. Classified from the error text by
/// [`classify`]; the first matching rule wins, so the order below is the
/// priority order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureKind {
    /// The operation was cancelled or failed and its rollback could not
    /// finish, leaving a relay pod, loopback alias or hosts entry behind.
    CleanupIncomplete,
    /// The user or a concurrent stop cancelled the operation.
    Cancelled,
    /// The forward was already running, here or in another kftray process.
    AlreadyRunning,
    /// A step did not finish in time (pod readiness, stream, allocation).
    Timeout,
    /// Choosing or claiming a loopback address.
    AddressAllocation,
    /// The local port could not be bound.
    PortInUse,
    /// Writing or removing the hosts-file alias.
    HostsFile,
    /// Building the TLS acceptor or the certificate.
    Tls,
    /// Reading or decoding the kubeconfig and its credentials.
    Kubeconfig,
    /// The service, pod or named port does not exist or selects nothing.
    TargetNotFound,
    /// The cluster refused or dropped the connection.
    Connection,
    /// Creating, reading or deleting the relay pod or its service.
    RelayPod,
    /// The configuration itself is invalid (no id, bad protocol or address).
    InvalidConfig,
    /// Reading or writing the local database.
    Database,
    /// Talking to the privileged helper.
    Helper,
    /// Cloning, fetching or authenticating against the git repository.
    Git,
    Other,
}

impl FailureKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::CleanupIncomplete => "cleanup_incomplete",
            Self::Cancelled => "cancelled",
            Self::AlreadyRunning => "already_running",
            Self::Timeout => "timeout",
            Self::AddressAllocation => "address_allocation",
            Self::PortInUse => "port_in_use",
            Self::HostsFile => "hosts_file",
            Self::Tls => "tls",
            Self::Kubeconfig => "kubeconfig",
            Self::TargetNotFound => "target_not_found",
            Self::Connection => "connection",
            Self::RelayPod => "relay_pod",
            Self::InvalidConfig => "invalid_config",
            Self::Database => "database",
            Self::Helper => "helper",
            Self::Git => "git",
            Self::Other => "other",
        }
    }
}

/// Maps an error message to a [`FailureKind`]. Only the kind leaves the
/// process; the message is used here and dropped.
pub fn classify(message: &str) -> FailureKind {
    const RULES: &[(FailureKind, &[&str])] = &[
        (FailureKind::CleanupIncomplete, &["cleanup incomplete"]),
        (FailureKind::Cancelled, &["cancelled", "canceled"]),
        (
            FailureKind::AlreadyRunning,
            &["already running", "being forwarded by another"],
        ),
        (FailureKind::Timeout, &["timed out", "timeout", "deadline"]),
        (
            FailureKind::AddressAllocation,
            &["allocat", "loopback", "still being released"],
        ),
        (
            FailureKind::PortInUse,
            &["address already in use", "address in use", "failed to bind"],
        ),
        (
            FailureKind::HostsFile,
            &["hosts file", "host entry", "hosts entry"],
        ),
        (
            FailureKind::Tls,
            &["tls", "certificate", "pkcs", "rustls", "acceptor"],
        ),
        (
            FailureKind::Kubeconfig,
            &[
                "kubeconfig",
                "client key",
                "exec plugin",
                "auth",
                "unauthorized",
                "forbidden",
            ],
        ),
        (
            FailureKind::TargetNotFound,
            &[
                "not found",
                "no pods",
                "has no selector",
                "no label selector",
                "no service name",
                "out of range",
                "kept changing",
            ],
        ),
        (
            FailureKind::Connection,
            &[
                "connection refused",
                "connection reset",
                "broken pipe",
                "dns",
                "resolve",
                "unreachable",
                "websocket",
                "stream",
            ],
        ),
        (
            FailureKind::RelayPod,
            &[
                "relay",
                "proxy pod",
                "proxy listener",
                "deployment",
                "pod has no ip",
            ],
        ),
        (
            FailureKind::InvalidConfig,
            &[
                "config has no id",
                "unsupported protocol",
                "invalid ip",
                "invalid port",
                "no label selector",
            ],
        ),
        (
            FailureKind::Database,
            &["sqlite", "database", "config_state"],
        ),
        (FailureKind::Helper, &["helper", "socket", "named pipe"]),
        (
            FailureKind::Git,
            &["git", "repository", "clone", "credential"],
        ),
    ];
    let message = message.to_ascii_lowercase();
    RULES
        .iter()
        .find(|(_, needles)| needles.iter().any(|needle| message.contains(needle)))
        .map_or(FailureKind::Other, |(kind, _)| *kind)
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

/// What the webview needs to report the same way this process does. `dsn` is
/// `None` when reporting is off for the whole process (debug build,
/// `DO_NOT_TRACK`), so the JS SDK is never initialised in that case.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FrontendContext {
    pub dsn: Option<&'static str>,
    pub release: &'static str,
    pub target: &'static str,
    pub run_id: &'static str,
    /// Current crash-reports consent; the webview keeps it in sync on toggles.
    pub enabled: bool,
    /// Current performance-data consent, likewise.
    pub performance_enabled: bool,
}

/// Only meaningful after [`init`]; before it `release` is empty.
pub fn frontend_context() -> FrontendContext {
    FrontendContext {
        dsn: reporting_allowed().then_some(DSN),
        release: RELEASE.get().copied().unwrap_or_default(),
        target: TARGET,
        run_id: &RUN_ID,
        enabled: ENABLED.load(Ordering::Relaxed),
        performance_enabled: *PERFORMANCE_ENABLED.lock(),
    }
}

/// `release` is `<app>@<version>`, e.g. `kftray@0.30.0`; the part before `@`
/// becomes the `app` tag so kftray and kftui events can be told apart.
pub fn init(release: &'static str) -> ClientInitGuard {
    LazyLock::force(&STARTED);
    let guard = sentry::init(client_options(release, reporting_allowed()));

    // Every other thread's hub is derived from the main hub, so tags set
    // there are inherited by tokio workers and by `measure`'s bound hubs.
    RELEASE.get_or_init(|| release);
    Hub::main().configure_scope(|scope| {
        scope.set_tag("app", app_name(release));
        scope.set_tag("target", TARGET);
        scope.set_tag("run_id", &*RUN_ID);
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

/// Stops new transaction captures before returning when disabled.
/// Transactions already queued in Sentry's transport can still be sent.
pub fn set_performance_enabled(enabled: bool) {
    *PERFORMANCE_ENABLED.lock() = enabled;
}

pub async fn load_setting() {
    apply(set_enabled, get_telemetry_enabled().await);
    apply(set_performance_enabled, get_performance_enabled().await);
}

pub async fn load_setting_with_mode(mode: DatabaseMode) {
    apply(set_enabled, get_telemetry_enabled_with_mode(mode).await);
    apply(
        set_performance_enabled,
        get_performance_enabled_with_mode(mode).await,
    );
}

/// Runs `future` as a transaction named after `operation`. Code awaited
/// inside it (not spawned from it) can attach child spans with [`phase`].
/// A failure is classified into a [`FailureKind`] and tagged on the transaction.
/// Cancellations get a cancelled status and an informational breadcrumb.
/// Other failures also produce an error event.
pub async fn measure<T, E: Display>(
    operation: Operation, future: impl Future<Output = Result<T, E>>,
) -> Result<T, E> {
    // A hub of its own so the transaction can be the scope's span for the
    // whole future even as tokio moves it between worker threads. Kept here
    // too: `bind_hub` only switches hubs while the future is polled, and the
    // failure event must be captured on this hub, while the transaction is
    // still its span, to carry the same trace id.
    let hub = Arc::new(Hub::new_from_top(Hub::current()));
    let transaction =
        hub.start_transaction(TransactionContext::new(operation.name(), operation.name()));
    hub.configure_scope(|scope| scope.set_span(Some(transaction.clone().into())));

    let result = future.bind_hub(Arc::clone(&hub)).await;

    match &result {
        Ok(_) => {
            transaction.set_status(SpanStatus::Ok);
            finish_unless_opted_out(transaction);
            push_breadcrumb(
                "portforward",
                format!("{} ok", operation.name()),
                Level::Info,
            );
        }
        Err(error) => {
            let kind = classify(&error.to_string());
            transaction.set_status(if kind == FailureKind::Cancelled {
                SpanStatus::Cancelled
            } else {
                SpanStatus::InternalError
            });
            transaction.set_tag("failure_kind", kind.name());
            report_failure_on(&hub, operation, kind);
            finish_unless_opted_out(transaction);
        }
    }
    result
}

/// Sentry samples a transaction when it starts, so a user who turns
/// performance data off while the operation runs is honored only here.
fn finish_unless_opted_out(transaction: Transaction) {
    let enabled = PERFORMANCE_ENABLED.lock();
    if *enabled {
        transaction.finish();
    }
}

/// Reports a failed operation as an error event titled
/// `<operation>: <kind>` and keeps it as a breadcrumb. For callers that see
/// failures inside an operation that still succeeded as a whole (a mixed
/// start batch); `measure` reports its own result on the operation's hub.
/// Cancellations leave an informational breadcrumb without an error event.
pub fn report_failure(operation: Operation, kind: FailureKind) {
    report_failure_on(&Hub::current(), operation, kind);
}

fn report_failure_on(hub: &Hub, operation: Operation, kind: FailureKind) {
    let cancelled = kind == FailureKind::Cancelled;
    push_breadcrumb(
        "portforward",
        format!("{} {}", operation.name(), kind.name()),
        if cancelled { Level::Info } else { Level::Error },
    );
    if cancelled {
        return;
    }
    let mut event = Event {
        level: Level::Error,
        exception: vec![Exception {
            ty: operation.name().into(),
            value: Some(kind.name().into()),
            mechanism: Some(Mechanism {
                ty: "operation".into(),
                handled: Some(true),
                ..Default::default()
            }),
            ..Default::default()
        }]
        .into(),
        ..Default::default()
    };
    event
        .tags
        .insert("operation".into(), operation.name().into());
    event.tags.insert("failure_kind".into(), kind.name().into());
    hub.capture_event(event);
}

/// Records the steps of an operation whose timing matters before the
/// performance setting can be read (app startup: the setting lives in the
/// database the startup is initialising). Steps are timed as they run and
/// the transaction is only built in [`Timeline::finish`], after the setting
/// is known, with the recorded timestamps.
pub struct Timeline {
    operation: Operation,
    started: SystemTime,
    phases: Vec<(Phase, SystemTime, SystemTime, bool)>,
}

impl Timeline {
    pub fn start(operation: Operation) -> Self {
        Self {
            operation,
            started: SystemTime::now(),
            phases: Vec::new(),
        }
    }

    /// Runs `future` as one step. A step that fails still ends the timeline
    /// normally; the failure shows as the span's status.
    pub async fn phase<T, E>(
        &mut self, phase: Phase, future: impl Future<Output = Result<T, E>>,
    ) -> Result<T, E> {
        let started = SystemTime::now();
        let result = future.await;
        self.phases
            .push((phase, started, SystemTime::now(), result.is_ok()));
        result
    }

    pub fn finish(self) {
        let transaction = sentry::start_transaction_with_timestamp(
            TransactionContext::new(self.operation.name(), self.operation.name()),
            self.started,
        );
        let failed = self.phases.iter().any(|(_, _, _, ok)| !ok);
        for (phase, started, ended, ok) in self.phases {
            let span =
                transaction.start_child_with_details(phase.name(), "", SpanId::default(), started);
            span.set_status(if ok {
                SpanStatus::Ok
            } else {
                SpanStatus::InternalError
            });
            span.finish_with_timestamp(ended);
        }
        transaction.set_status(if failed {
            SpanStatus::InternalError
        } else {
            SpanStatus::Ok
        });
        finish_unless_opted_out(transaction);
    }
}

/// `allowed` is the process-wide gate (`DO_NOT_TRACK`, debug builds). It is
/// enforced where events and transactions are emitted, not only on the DSN:
/// `sentry::init` fills an empty DSN from `SENTRY_DSN`, so a `None` DSN alone
/// would let the environment re-enable reporting the user turned off.
fn client_options(release: &'static str, allowed: bool) -> ClientOptions {
    let mut options = ClientOptions::new()
        .traces_sampler(move |_| sample_rate(allowed && *PERFORMANCE_ENABLED.lock()));
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

fn apply(set_enabled: fn(bool), setting: SettingResult) {
    match setting {
        Ok(enabled) => set_enabled(enabled == Some(true)),
        Err(e) => warn!("Failed to read telemetry setting: {e}"),
    }
}

/// Fixed for the life of the process: `DO_NOT_TRACK` is read once at init,
/// so a transaction sampled in is never from a run that opted out.
fn reporting_allowed() -> bool {
    !cfg!(debug_assertions) && std::env::var_os("DO_NOT_TRACK").is_none()
}

/// Something that happened in this run, kept for the next error report. Fixed
/// names only; nothing about which configuration or window it concerned.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppEvent {
    WindowShown,
    WindowHidden,
    LogViewerOpened,
    UpdateCheck,
    UpdateInstall,
    TrayOnline,
    TrayOffline,
    ExitRequested,
}

impl AppEvent {
    fn name(self) -> &'static str {
        match self {
            Self::WindowShown => "window.shown",
            Self::WindowHidden => "window.hidden",
            Self::LogViewerOpened => "logs.opened",
            Self::UpdateCheck => "update.check",
            Self::UpdateInstall => "update.install",
            Self::TrayOnline => "tray.online",
            Self::TrayOffline => "tray.offline",
            Self::ExitRequested => "app.exit_requested",
        }
    }
}

/// Records an [`AppEvent`] as a breadcrumb for the next error report.
pub fn breadcrumb(event: AppEvent) {
    push_breadcrumb("app", event.name().to_string(), Level::Info);
}

fn push_breadcrumb(category: &'static str, message: String, level: Level) {
    let crumb = Breadcrumb {
        category: Some(category.into()),
        message: Some(message),
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
    fn scrub_attaches_recorded_events_as_breadcrumbs() {
        report_failure(Operation::StartForward, FailureKind::Timeout);
        breadcrumb(AppEvent::LogViewerOpened);

        let event = scrub(true, event_from("laptop")).unwrap();
        let messages: Vec<_> = event
            .breadcrumbs
            .values
            .iter()
            .map(|crumb| {
                (
                    crumb.category.as_deref(),
                    crumb.message.as_deref(),
                    crumb.level,
                )
            })
            .collect();

        assert!(messages.contains(&(
            Some("portforward"),
            Some("portforward.start timeout"),
            Level::Error
        )));
        assert!(messages.contains(&(Some("app"), Some("logs.opened"), Level::Info)));
    }

    #[test]
    fn breadcrumbs_are_bounded() {
        for _ in 0..(MAX_BREADCRUMBS * 2) {
            breadcrumb(AppEvent::WindowShown);
        }

        assert!(BREADCRUMBS.lock().len() <= MAX_BREADCRUMBS);
    }

    #[test]
    fn classify_picks_the_first_matching_rule() {
        assert_eq!(
            classify("Startup cancelled for config 3"),
            FailureKind::Cancelled
        );
        assert_eq!(
            classify("Timed out waiting for proxy listener in pod kftray-proxy-x"),
            FailureKind::Timeout
        );
        assert_eq!(
            classify(
                "Failed to start TCP port forwarding for service api: Service 'api' not found: ApiError"
            ),
            FailureKind::TargetNotFound
        );
        assert_eq!(
            classify("Failed to bind TCP listener to 127.0.0.1:8080: Address already in use"),
            FailureKind::PortInUse
        );
        assert_eq!(
            classify("Failed to create configuration from kubeconfig: exec plugin"),
            FailureKind::Kubeconfig
        );
        assert_eq!(classify("something unexpected"), FailureKind::Other);
    }

    #[test]
    fn failed_measure_reports_the_kind_not_the_message() {
        let envelopes = envelopes_with_consent(true, || {
            let result: Result<(), String> =
                futures::executor::block_on(measure(Operation::StartForward, async {
                    Err("Service 'secret-svc' not found in namespace prod".to_string())
                }));
            assert!(result.is_err());
        });
        let json = format!("{envelopes:?}");

        assert!(!json.contains("secret-svc"), "{json}");
        assert!(!json.contains("prod"), "{json}");
        let event = envelopes
            .iter()
            .flat_map(|envelope| envelope.items())
            .find_map(|item| match item {
                EnvelopeItem::Event(event) => Some(event),
                _ => None,
            })
            .unwrap();
        let exception = &event.exception.values[0];
        assert_eq!(exception.ty, "portforward.start");
        assert_eq!(exception.value.as_deref(), Some("target_not_found"));
        assert_eq!(event.tags["failure_kind"], "target_not_found");
        let transaction = captured_transaction(&envelopes);
        assert_eq!(transaction.tags["failure_kind"], "target_not_found");
        // Linked to the measured transaction, so GlitchTip shows them together.
        let trace_id_of =
            |contexts: &sentry::protocol::Map<String, sentry::protocol::Context>| match contexts
                .get("trace")
            {
                Some(sentry::protocol::Context::Trace(trace)) => Some(trace.trace_id),
                _ => None,
            };
        assert!(trace_id_of(&event.contexts).is_some());
        assert_eq!(
            trace_id_of(&event.contexts),
            trace_id_of(&transaction.contexts)
        );
    }

    #[test]
    fn cancelled_measure_keeps_the_error_and_records_a_cancelled_transaction() {
        let envelopes = envelopes_with_consent(true, || {
            let result: Result<(), &str> =
                futures::executor::block_on(measure(Operation::AutoStart, async {
                    Err("Startup cancelled for config 2")
                }));
            assert_eq!(result, Err("Startup cancelled for config 2"));
        });

        assert!(
            envelopes
                .iter()
                .flat_map(|envelope| envelope.items())
                .all(|item| !matches!(item, EnvelopeItem::Event(_))),
            "cancellation must not create an error issue: {envelopes:?}"
        );
        let transaction = captured_transaction(&envelopes);
        assert_eq!(transaction.tags["failure_kind"], "cancelled");
        assert_eq!(
            transaction
                .contexts
                .get("trace")
                .and_then(|context| match context {
                    sentry::protocol::Context::Trace(trace) => trace.status,
                    _ => None,
                }),
            Some(SpanStatus::Cancelled)
        );
    }

    #[test]
    fn turning_performance_off_mid_measure_drops_the_transaction_only() {
        let item_kinds = |outcome: Result<(), String>| {
            let expected = outcome.clone();
            let envelopes = envelopes_with_consent(true, || {
                let result = futures::executor::block_on(measure(Operation::StartForward, async {
                    set_performance_enabled(false);
                    outcome
                }));
                assert_eq!(result, expected);
            });
            envelopes
                .iter()
                .flat_map(|envelope| envelope.items())
                .map(|item| match item {
                    EnvelopeItem::Event(_) => "event",
                    EnvelopeItem::Transaction(_) => "transaction",
                    _ => "other",
                })
                .collect::<Vec<_>>()
        };

        assert_eq!(item_kinds(Ok(())), Vec::<&str>::new());
        assert_eq!(
            item_kinds(Err("Service 'api' not found".to_string())),
            vec!["event"]
        );
    }

    #[test]
    fn performance_opt_out_waits_for_transaction_capture() {
        use std::sync::mpsc;
        use std::time::Duration;

        struct PausedTransport {
            entered: mpsc::SyncSender<()>,
            resume: Mutex<mpsc::Receiver<()>>,
            opted_out: Arc<AtomicBool>,
            captured_after_opt_out: Arc<AtomicBool>,
            captured: Arc<sentry::test::TestTransport>,
        }

        impl sentry::Transport for PausedTransport {
            fn send_envelope(&self, envelope: sentry::Envelope) {
                self.entered.send(()).unwrap();
                self.resume
                    .lock()
                    .recv_timeout(Duration::from_secs(5))
                    .unwrap();
                self.captured_after_opt_out
                    .store(self.opted_out.load(Ordering::SeqCst), Ordering::SeqCst);
                self.captured.send_envelope(envelope);
            }
        }

        let opted_out = Arc::new(AtomicBool::new(false));
        let captured_after_opt_out = Arc::new(AtomicBool::new(false));
        let captured = sentry::test::TestTransport::new();
        envelopes_with_consent(true, || {
            let (entered_tx, entered_rx) = mpsc::sync_channel(1);
            let (resume_tx, resume_rx) = mpsc::sync_channel(1);
            let transport = Arc::new(PausedTransport {
                entered: entered_tx,
                resume: Mutex::new(resume_rx),
                opted_out: Arc::clone(&opted_out),
                captured_after_opt_out: Arc::clone(&captured_after_opt_out),
                captured: Arc::clone(&captured),
            });
            let client = Arc::new(sentry::Client::from_config(
                client_options("kftray@test", true).transport(transport),
            ));
            let hub = Arc::new(Hub::new(Some(client), Default::default()));
            std::thread::scope(|threads| {
                let measuring_hub = Arc::clone(&hub);
                let measuring = threads.spawn(move || {
                    Hub::run(measuring_hub, || {
                        futures::executor::block_on(measure(Operation::StartForward, async {
                            Ok::<_, &str>(())
                        }))
                    })
                });
                entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                let (started_tx, started_rx) = mpsc::sync_channel(1);
                let (done_tx, done_rx) = mpsc::sync_channel(1);
                let opting_out = threads.spawn(move || {
                    started_tx.send(()).unwrap();
                    set_performance_enabled(false);
                    opted_out.store(true, Ordering::SeqCst);
                    done_tx.send(()).unwrap();
                });
                started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                // Let opt-out race with a capture paused inside Sentry's transport.
                let _ = done_rx.recv_timeout(Duration::from_secs(1));
                resume_tx.send(()).unwrap();
                assert_eq!(measuring.join().unwrap(), Ok(()));
                opting_out.join().unwrap();

                Hub::run(hub, || {
                    let result =
                        futures::executor::block_on(measure(Operation::StopForward, async {
                            Ok::<_, &str>(())
                        }));
                    assert_eq!(result, Ok(()));
                });
            });
        });

        assert!(!captured_after_opt_out.load(Ordering::SeqCst));
        let envelopes = captured.fetch_and_clear_envelopes();
        assert_eq!(envelopes.len(), 1);
        assert_eq!(
            captured_transaction(&envelopes).name.as_deref(),
            Some("portforward.start")
        );
    }

    #[test]
    fn cancelled_startup_with_incomplete_cleanup_still_reports_an_error() {
        let envelopes = envelopes_with_consent(true, || {
            let result: Result<(), &str> =
                futures::executor::block_on(measure(Operation::AutoStart, async {
                    Err("Startup cancelled for config 2; cleanup incomplete: relay delete failed")
                }));
            assert!(result.is_err());
        });
        let event = envelopes
            .iter()
            .flat_map(|envelope| envelope.items())
            .find_map(|item| match item {
                EnvelopeItem::Event(event) => Some(event),
                _ => None,
            })
            .unwrap();
        assert_eq!(event.tags["failure_kind"], "cleanup_incomplete");
        let transaction = captured_transaction(&envelopes);
        assert_eq!(transaction.tags["failure_kind"], "cleanup_incomplete");
        assert_eq!(
            transaction
                .contexts
                .get("trace")
                .and_then(|context| match context {
                    sentry::protocol::Context::Trace(trace) => trace.status,
                    _ => None,
                }),
            Some(SpanStatus::InternalError)
        );
    }

    #[test]
    fn cancelled_batch_item_does_not_hide_another_failure() {
        let envelopes = envelopes_with_consent(true, || {
            report_failure(Operation::StartForward, FailureKind::Cancelled);
            report_failure(Operation::StartForward, FailureKind::Timeout);
        });
        let failures: Vec<_> = envelopes
            .iter()
            .flat_map(|envelope| envelope.items())
            .filter_map(|item| match item {
                EnvelopeItem::Event(event) => Some(event.tags["failure_kind"].as_str()),
                _ => None,
            })
            .collect();

        assert_eq!(failures, ["timeout"]);
    }

    #[test]
    fn timeline_builds_a_transaction_from_recorded_steps() {
        let envelopes = envelopes_with_consent(true, || {
            futures::executor::block_on(async {
                let mut timeline = Timeline::start(Operation::AppStartup);
                let _: Result<(), ()> = timeline.phase(Phase::DbInit, async { Ok(()) }).await;
                let _: Result<(), ()> = timeline.phase(Phase::DbMigrate, async { Err(()) }).await;
                timeline.finish();
            });
        });
        let transaction = captured_transaction(&envelopes);
        let spans: Vec<_> = transaction
            .spans
            .iter()
            .map(|span| (span.op.as_deref(), span.status))
            .collect();

        assert_eq!(transaction.name.as_deref(), Some("app.startup"));
        assert_eq!(
            spans,
            vec![
                (Some("db.init"), Some(SpanStatus::Ok)),
                (Some("db.migrate"), Some(SpanStatus::InternalError)),
            ]
        );
        assert_eq!(
            transaction.contexts.get("trace").and_then(|c| match c {
                sentry::protocol::Context::Trace(trace) => trace.status,
                _ => None,
            }),
            Some(SpanStatus::InternalError)
        );
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
        let envelopes = envelopes_with_consent(true, || {
            let result: Result<(), String> =
                futures::executor::block_on(measure(Operation::StopForward, async {
                    Err("boom".to_string())
                }));
            assert!(result.is_err());
        });
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
        let envelopes = envelopes_with_consent(true, || {
            let result: Result<(), String> =
                futures::executor::block_on(measure(Operation::StartForward, async {
                    let connect = phase(Phase::Connect);
                    futures::future::ready(()).await;
                    drop(connect);
                    let listen = phase(Phase::Listen);
                    listen.fail();
                    Ok(())
                }));
            assert!(result.is_ok());
        });
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

    /// The consent flags are process globals; tests that flip them must not
    /// overlap.
    static CONSENT_LOCK: Mutex<()> = Mutex::new(());

    /// Runs `emit` with both consent flags on, under a client that has a DSN
    /// (the test transport sets one, like `SENTRY_DSN` would), and returns
    /// what the client let through.
    fn envelopes_with_consent(allowed: bool, emit: impl FnOnce()) -> Vec<sentry::Envelope> {
        let _consent = CONSENT_LOCK.lock();
        ENABLED.store(true, Ordering::Relaxed);
        set_performance_enabled(true);
        let envelopes = sentry::test::with_captured_envelopes_options(
            emit,
            client_options("kftray@test", allowed),
        );
        ENABLED.store(false, Ordering::Relaxed);
        set_performance_enabled(false);
        envelopes
    }

    fn emit_one_of_each() {
        let result: Result<(), String> =
            futures::executor::block_on(measure(Operation::StartForward, async { Ok(()) }));
        assert!(result.is_ok());
        sentry::capture_event(Event {
            level: Level::Error,
            ..Default::default()
        });
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
    fn frontend_context_mirrors_the_process_gate_and_consent() {
        let _consent = CONSENT_LOCK.lock();
        RELEASE.get_or_init(|| "kftray@test");
        set_enabled(true);
        let context = frontend_context();
        set_enabled(false);

        // Debug builds (tests included) never hand out a DSN.
        assert_eq!(context.dsn, None);
        assert_eq!(context.release, "kftray@test");
        assert_eq!(context.target, TARGET);
        assert_eq!(context.run_id, *RUN_ID);
        assert!(context.enabled);
    }
}
