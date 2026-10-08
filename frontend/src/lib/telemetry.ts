import { queryOptions } from '@tanstack/react-query'
import { invoke } from '@tauri-apps/api/core'

export type ErrorReport = {
  name: string
  message: string
  stack: string | null
}

export const telemetryQuery = queryOptions({
  queryKey: ['telemetry-enabled'],
  queryFn: () => invoke<boolean | null>('get_telemetry_enabled'),
})

export const errorReport = (reason: unknown): ErrorReport | null =>
  reason instanceof Error
    ? {
        name: reason.name,
        message: reason.message,
        stack: reason.stack ?? null,
      }
    : null

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
