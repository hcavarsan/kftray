import { errorMessage } from '@/lib/errors'

import { toaster } from './toaster'

export interface ErrorToastMeta {
  errorToast?: {
    id?: string
    title: string
    description?: string
    duration?: number
  }
}

export const showErrorToast = (
  error: unknown,
  meta: ErrorToastMeta | undefined,
) => {
  const toast = meta?.errorToast

  if (toast) {
    toaster.error({
      ...(toast.id !== undefined && { id: toast.id }),
      title: toast.title,
      description: toast.description ?? errorMessage(error),
      duration: toast.duration,
    })
  }
}
