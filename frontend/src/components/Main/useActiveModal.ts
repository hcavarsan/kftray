import { useCallback, useRef, useState } from 'react'

import { useQueryClient } from '@tanstack/react-query'

import { invoke } from '@/lib/tauri'
import type { StoredConfig } from '@/types'

type ActiveModal =
  | { type: 'config'; initialConfig: StoredConfig | null; isEdit: boolean }
  | {
      type:
        | 'autoImport'
        | 'gitSync'
        | 'helper'
        | 'serverResources'
        | 'settings'
        | 'shortcuts'
    }
  | null

export function useActiveModal() {
  const [session, setSession] = useState<{
    activeModal: ActiveModal
    modalKey: number
  }>({ activeModal: null, modalKey: 0 })
  const request = useRef(0)
  const queryClient = useQueryClient()
  const setActiveModal = useCallback((activeModal: ActiveModal) => {
    request.current += 1
    setSession({ activeModal, modalKey: request.current })
  }, [])

  const openConfig = useCallback(
    async (id: number, isEdit: boolean) => {
      const currentRequest = ++request.current
      const config = await queryClient
        .fetchQuery({
          queryKey: ['config', id],
          queryFn: () => invoke<StoredConfig>('get_config_cmd', { id }),
          staleTime: 0,
          meta: { errorToast: { title: 'Failed to load configuration' } },
        })
        .catch(() => null)

      if (!config || currentRequest !== request.current) {
        return
      }
      setActiveModal({
        type: 'config',
        isEdit,
        initialConfig: isEdit
          ? config
          : { ...config, id: 0, alias: `${config.alias ?? ''}-copy` },
      })
    },
    [queryClient, setActiveModal],
  )

  return { ...session, setActiveModal, openConfig }
}
