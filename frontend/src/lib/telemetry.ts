import type { ErrorEvent, Event, Exception, StackFrame } from '@sentry/browser'
import * as Sentry from '@sentry/browser'
import { queryOptions } from '@tanstack/react-query'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

import type { ProblemReportContext } from './problemReports'

export const telemetryQuery = queryOptions({
  queryKey: ['telemetry-enabled'],
  queryFn: () => invoke<boolean | null>('get_telemetry_enabled'),
})

export const performanceQuery = queryOptions({
  queryKey: ['performance-enabled'],
  queryFn: () => invoke<boolean | null>('get_performance_enabled'),
})

export type Consent = 'crashReports' | 'performance'

// Mirrors kftray_telemetry::FrontendContext. `dsn` is null when the Rust side
// will not report either (debug build, DO_NOT_TRACK), so the SDK is never
// started in that case and nothing leaves the webview.
type TelemetryContext = {
  dsn: string | null
  release: string
  target: string
  run_id: string
  enabled: boolean
  performance_enabled: boolean
}

// The Rust side owns the stored choices and broadcasts every change; these
// are the live copies the SDK in this window consults. Each webview (main,
// logs) runs its own copy, so a toggle in one window reaches the others
// through the broadcast rather than through the setters below.
let crashReportsConsent = false
let performanceConsent = false

// Match the constants in commands/telemetry.rs.
const TELEMETRY_ENABLED_CHANGED = 'telemetry-enabled-changed'
const PERFORMANCE_ENABLED_CHANGED = 'performance-enabled-changed'

export const setCrashReportsConsent = async (enabled: boolean) => {
  await invoke('update_telemetry_enabled', { enabled })
  crashReportsConsent = enabled
}

export const setPerformanceConsent = async (enabled: boolean) => {
  await invoke('update_performance_enabled', { enabled })
  performanceConsent = enabled
}

// Files served from the app bundle: tauri://localhost on macOS and Linux,
// https://tauri.localhost on Windows (useHttpsScheme). Frames from anywhere
// else are dropped rather than sent.
const APP_ORIGIN = /^(?:tauri:\/\/localhost|https?:\/\/tauri\.localhost)\//

const keepFrame = (frame: StackFrame): StackFrame | null => {
  const location = frame.abs_path ?? frame.filename
  if (!location || !APP_ORIGIN.test(location)) {
    return null
  }
  return {
    abs_path: location,
    filename: location,
    lineno: frame.lineno,
    colno: frame.colno,
    function: frame.function,
    in_app: true,
  }
}

// Reduces an event to what the Rust side also sends for a panic: the error
// type and code locations. The message is never sent: it can carry a cluster
// address or a resource name, for example when a failed port forward
// rethrows kubectl's stderr. The page URL and user agent, breadcrumbs and
// any attached context go too. Rejections that were not an Error have no
// type or location of their own (the SDK synthesises one from the value)
// and are dropped entirely.
export const scrub = (event: ErrorEvent): ErrorEvent | null => {
  const exceptions = (event.exception?.values ?? []).filter(
    exception => !exception.mechanism?.synthetic,
  )
  if (exceptions.length === 0) {
    return null
  }
  return {
    type: event.type,
    event_id: event.event_id,
    timestamp: event.timestamp,
    level: event.level,
    platform: event.platform,
    sdk: event.sdk,
    release: event.release,
    environment: event.environment,
    tags: event.tags,
    debug_meta: event.debug_meta,
    exception: {
      values: exceptions.map(
        (exception): Exception => ({
          type: exception.type,
          mechanism: exception.mechanism,
          stacktrace: {
            frames: (exception.stacktrace?.frames ?? []).flatMap(
              frame => keepFrame(frame) ?? [],
            ),
          },
        }),
      ),
    },
  }
}

// `@sentry/browser` re-exports `Event` but not the transaction variant.
export type TransactionEvent = Event & { type: 'transaction' }

// Reduces a transaction to its timing: name, operation, status, spans. The
// request (page URL, user agent), breadcrumbs and any other context go.
export const scrubTransaction = (
  event: TransactionEvent,
): TransactionEvent => ({
  type: event.type,
  event_id: event.event_id,
  transaction: event.transaction,
  start_timestamp: event.start_timestamp,
  timestamp: event.timestamp,
  platform: event.platform,
  sdk: event.sdk,
  release: event.release,
  environment: event.environment,
  tags: event.tags,
  contexts: { trace: event.contexts?.trace },
  spans: (event.spans ?? []).map(span => ({
    span_id: span.span_id,
    parent_span_id: span.parent_span_id,
    trace_id: span.trace_id,
    op: span.op,
    description: span.description,
    status: span.status,
    start_timestamp: span.start_timestamp,
    timestamp: span.timestamp,
    // Required by the type; span data is where arguments would end up.
    data: {},
  })),
})

// Times one call to the Rust side as a `tauri.invoke` transaction named
// after the command. Sampled out (and therefore free) without performance
// consent or before the SDK is started.
export const invokeSpan = <T>(command: string, run: () => Promise<T>) =>
  Sentry.startSpan({ name: command, op: 'tauri.invoke' }, run)

// A rejected invoke, reported as the fixed type `InvokeError` tagged with
// the command name. The rejection value is a message from Rust that can
// carry a cluster or resource name, so it is not attached.
export class InvokeError extends Error {
  constructor(readonly command: string) {
    super(command)
    this.name = 'InvokeError'
  }
}

export const captureInvokeFailure = (command: string): ProblemReportContext => {
  if (!crashReportsConsent) {
    return undefined
  }
  const client = Sentry.getClient()
  if (!client?.getDsn()) {
    return undefined
  }
  const eventId = crypto.randomUUID().replaceAll('-', '')
  const delivery = new Promise<boolean>(resolve => {
    const timeout = setTimeout(() => {
      unsubscribe()
      resolve(false)
    }, 15_000)
    const unsubscribe = client.on('afterSendEvent', (event, response) => {
      if (event.event_id !== eventId) {
        return
      }
      clearTimeout(timeout)
      unsubscribe()
      const status = response.statusCode
      resolve(status !== undefined && status >= 200 && status < 300)
    })
    Sentry.captureException(new InvokeError(command), {
      event_id: eventId,
      captureContext: { tags: { command } },
    })
  })
  return { eventId, delivery }
}

// Starts the SDK once the Rust side has said whether reporting is allowed.
// Errors thrown before that answer arrives are not reported.
export const initCrashReporting = async () => {
  // Subscribed before the context is read: a toggle in another window that
  // lands between the two would otherwise be overwritten by the stale
  // values in the context.
  let crashBroadcast: boolean | undefined
  let performanceBroadcast: boolean | undefined
  await Promise.all([
    listen<boolean>(TELEMETRY_ENABLED_CHANGED, event => {
      crashBroadcast = event.payload
      crashReportsConsent = event.payload
    }),
    listen<boolean>(PERFORMANCE_ENABLED_CHANGED, event => {
      performanceBroadcast = event.payload
      performanceConsent = event.payload
    }),
  ]).catch(() => undefined)

  const context = await invoke<TelemetryContext>('get_telemetry_context').catch(
    () => null,
  )
  if (!context?.dsn) {
    return
  }
  crashReportsConsent = crashBroadcast ?? context.enabled
  performanceConsent = performanceBroadcast ?? context.performance_enabled

  Sentry.init({
    dsn: context.dsn,
    release: context.release,
    environment: 'production',
    // Client reports describe dropped events; they bypass beforeSend.
    sendClientReports: false,
    // Without this the SDK tags every envelope `infer_ip: auto`, asking the
    // server to record the client address.
    dataCollection: { userInfo: false },
    // Only the handlers that catch errors; no console, DOM, fetch or
    // navigation breadcrumbs, no session tracking, no automatic page-load
    // or navigation transactions: the only spans are the invoke timings.
    defaultIntegrations: false,
    integrations: [
      Sentry.globalHandlersIntegration(),
      Sentry.browserApiErrorsIntegration(),
      Sentry.linkedErrorsIntegration(),
      Sentry.dedupeIntegration(),
    ],
    initialScope: {
      tags: {
        app: 'kftray',
        surface: 'webview',
        target: context.target,
        run_id: context.run_id,
      },
    },
    beforeBreadcrumb: () => null,
    beforeSend: event => (crashReportsConsent ? scrub(event) : null),
    // Decided when each span starts, so a toggle takes effect on the next
    // call rather than on restart.
    tracesSampler: () => (performanceConsent ? 1 : 0),
    // Whole transactions, the shape GlitchTip ingests and the Rust SDK sends,
    // rather than the SDK's default streamed span batches. Also the only
    // mode in which beforeSendTransaction runs.
    traceLifecycle: 'static',
    beforeSendTransaction: event =>
      performanceConsent ? scrubTransaction(event) : null,
  })
}
