import { useMutation } from '@tanstack/react-query'

import { toaster } from '@/components/ui/toaster'

interface CopyFeedback {
  title: string
  description: (text: string) => string
}

export function useCopyToClipboard(feedback?: CopyFeedback) {
  return useMutation({
    mutationFn: (text: string) => navigator.clipboard.writeText(text),
    onSuccess: (_data, text) => {
      if (feedback) {
        toaster.success({
          title: feedback.title,
          description: feedback.description(text),
          duration: 2000,
        })
      }
    },
    meta: { errorToast: { title: 'Copy failed', duration: 3000 } },
  })
}
