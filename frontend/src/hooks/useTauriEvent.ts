import { useEffect, useEffectEvent } from 'react'

import { type EventCallback, listen } from '@tauri-apps/api/event'

import { toaster } from '@/components/ui/toaster'
import { errorMessage } from '@/lib/errors'

export function useTauriEvent<T>(event: string, handler: EventCallback<T>) {
  const onEvent = useEffectEvent(handler)

  useEffect(() => {
    let disposed = false
    let unlisten: (() => void) | undefined

    listen<T>(event, payload => onEvent(payload))
      .then(fn => {
        if (disposed) {
          fn()
        } else {
          unlisten = fn
        }
      })
      .catch(error => {
        toaster.error({
          title: `Failed to subscribe to ${event}`,
          description: errorMessage(error),
        })
      })

    return () => {
      disposed = true
      unlisten?.()
    }
  }, [event])
}
