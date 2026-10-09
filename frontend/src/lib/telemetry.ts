import type { ErrorEvent, Exception, StackFrame } from '@sentry/browser'
import * as Sentry from '@sentry/browser'
import { queryOptions } from '@tanstack/react-query'
import { invoke } from '@tauri-apps/api/core'

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
}

// The Rust side owns the stored choice; this is the live copy the SDK
// consults, so a toggle in Settings takes effect without a restart.
let crashReportsConsent = false

export const setCrashReportsConsent = async (enabled: boolean) => {
  await invoke('update_telemetry_enabled', { enabled })
  crashReportsConsent = enabled
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

// Starts the SDK once the Rust side has said whether reporting is allowed.
// Errors thrown before that answer arrives are not reported.
export const initCrashReporting = async () => {
  const context = await invoke<TelemetryContext>('get_telemetry_context').catch(
    () => null,
  )
  if (!context?.dsn) {
    return
  }
  crashReportsConsent = context.enabled

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
    // navigation breadcrumbs, no session tracking.
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
  })
}
