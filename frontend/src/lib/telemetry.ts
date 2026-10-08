import { queryOptions } from '@tanstack/react-query'
import { invoke } from '@tauri-apps/api/core'

export type ErrorReport = {
  name: string
  stack: string | null
}

export const telemetryQuery = queryOptions({
  queryKey: ['telemetry-enabled'],
  queryFn: () => invoke<boolean | null>('get_telemetry_enabled'),
})

export const performanceQuery = queryOptions({
  queryKey: ['performance-enabled'],
  queryFn: () => invoke<boolean | null>('get_performance_enabled'),
})

export type Consent = 'crashReports' | 'performance'

// "url:line:column" of a file served from the app bundle: tauri://localhost
// on macOS and Linux, https://tauri.localhost on Windows (useHttpsScheme).
const ASSET_LOCATION =
  /(?:tauri:\/\/localhost|https?:\/\/tauri\.localhost)(?:\/[\w.-]+)+:\d+:\d+/

// The message is never sent: it can carry a cluster address or a resource
// name, for example when a failed port forward rethrows kubectl's stderr.
// V8 starts the stack with "name: message", and a message can span lines
// that look like frames, so the header is cut off first and then only the
// bundle locations are taken from each line. Function names and any other
// text on a line are dropped.
export const errorReport = (reason: unknown): ErrorReport | null => {
  if (!(reason instanceof Error)) {
    return null
  }

  const header = reason.message
    ? `${reason.name}: ${reason.message}`
    : reason.name
  const stack = reason.stack ?? ''
  const locations = (
    stack.startsWith(header) ? stack.slice(header.length) : stack
  )
    .split('\n')
    .flatMap(line => line.match(ASSET_LOCATION)?.[0] ?? [])

  return {
    name: reason.name,
    stack: locations.length > 0 ? locations.join('\n') : null,
  }
}

const reportError = (reason: unknown) => {
  const report = errorReport(reason)

  if (report) {
    invoke('report_error', report).catch(() => undefined)
  }
}

export const installErrorReporting = () => {
  window.addEventListener('error', event => reportError(event.error))
  window.addEventListener('unhandledrejection', event =>
    reportError(event.reason),
  )
}
