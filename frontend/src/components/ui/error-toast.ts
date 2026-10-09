import { errorMessage } from '@/lib/errors'
import {
  getProblemReportContext,
  requestProblemReport,
} from '@/lib/problemReports'

import { toaster } from './toaster'

const REPORT_ACTION_DURATION = 10000

export interface ErrorToastMeta {
  errorToast?: {
    id?: string
    title: string
    description?: string
    duration?: number
    report?: boolean
  }
}

export const showErrorToast = (
  error: unknown,
  meta: ErrorToastMeta | undefined,
) => {
  const toast = meta?.errorToast

  if (toast) {
    const reportContext =
      toast.report === false ? undefined : getProblemReportContext(error)

    toaster.error({
      ...(toast.id !== undefined && { id: toast.id }),
      title: toast.title,
      description: toast.description ?? errorMessage(error),
      duration: reportContext
        ? Math.max(toast.duration ?? 0, REPORT_ACTION_DURATION)
        : toast.duration,
      ...(reportContext && {
        action: {
          label: 'Report',
          onClick: () => requestProblemReport(reportContext),
        },
      }),
    })
  }
}
