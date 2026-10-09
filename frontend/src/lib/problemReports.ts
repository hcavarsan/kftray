import { queryOptions } from '@tanstack/react-query'
import { invoke } from '@tauri-apps/api/core'

import { errorMessage } from './errors'

export type ProblemReportContext =
  | { eventId: string; delivery: Promise<boolean> }
  | undefined

const reportContexts = new WeakMap<object, ProblemReportContext>()
const reportListeners = new Set<(context: ProblemReportContext) => void>()

export const problemReportAvailabilityQuery = queryOptions({
  queryKey: ['problem-report-available'],
  queryFn: async () => {
    const context = await invoke<{ dsn: string | null }>(
      'get_telemetry_context',
    )
    return context.dsn !== null
  },
})

export const attachProblemReportContext = (
  error: unknown,
  context: ProblemReportContext,
): unknown => {
  if (!context) {
    return error
  }
  const failure =
    error !== null && typeof error === 'object'
      ? error
      : new Error(errorMessage(error), { cause: error })
  reportContexts.set(failure, context)
  return failure
}

export const getProblemReportContext = (
  error: unknown,
): ProblemReportContext =>
  error !== null && typeof error === 'object'
    ? reportContexts.get(error)
    : undefined

export const requestProblemReport = (context?: ProblemReportContext) => {
  for (const listener of reportListeners) {
    listener(context)
  }
}

export const subscribeToProblemReports = (
  listener: (context: ProblemReportContext) => void,
) => {
  reportListeners.add(listener)
  return () => {
    reportListeners.delete(listener)
  }
}

export const submitProblemReport = async ({
  reportId,
  message,
  email,
  context,
}: {
  reportId: string
  message: string
  email?: string
  context?: ProblemReportContext
}): Promise<void> => {
  if (context && !(await context.delivery)) {
    throw new Error(
      'The error could not be delivered. Use Report a problem from the menu to send a separate report.',
    )
  }
  await invoke('submit_problem_report', {
    report: {
      report_id: reportId,
      message: message.trim(),
      email: email?.trim() || null,
      event_id: context?.eventId ?? null,
    },
  })
}
