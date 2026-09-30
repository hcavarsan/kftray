import {
  queryOptions,
  useMutation,
  useQuery,
  useQueryClient,
} from '@tanstack/react-query'
import { invoke } from '@tauri-apps/api/core'

import { toaster } from '@/components/ui/toaster'
import { useTauriEvent } from '@/hooks/useTauriEvent'

export interface Shortcut {
  id: number
  name: string
  shortcut_key: string
  action_type: string
  action_data?: string
  enabled: boolean
}

interface PlatformStatus {
  platform: string
  needs_permission_fix: boolean
  current_implementation: string
  can_fix_permissions: boolean
}

export const shortcutsQuery = queryOptions({
  queryKey: ['shortcuts'],
  queryFn: () => invoke<Shortcut[]>('get_shortcuts'),
})

const platformStatusQuery = queryOptions({
  queryKey: ['platform-status'],
  queryFn: () => invoke<PlatformStatus>('get_platform_status'),
})

export function useGlobalShortcuts() {
  const queryClient = useQueryClient()
  const shortcuts = useQuery(shortcutsQuery)
  const { data: platformStatus } = useQuery(platformStatusQuery)

  useTauriEvent<PlatformStatus>('platform-status-update', event =>
    queryClient.setQueryData(platformStatusQuery.queryKey, event.payload),
  )

  const deleteShortcut = useMutation({
    mutationFn: (id: number) => invoke('delete_shortcut', { id }),
    onSuccess: () =>
      toaster.success({
        title: 'Deleted',
        description: 'Shortcut deleted successfully',
        duration: 3000,
      }),
    onError: () =>
      toaster.error({
        title: 'Error',
        description: 'Failed to delete shortcut',
        duration: 3000,
      }),
    onSettled: () =>
      queryClient.invalidateQueries({ queryKey: shortcutsQuery.queryKey }),
  })

  const fixPermissions = useMutation({
    mutationFn: () => invoke<string>('try_fix_platform_permissions'),
    onSuccess: () =>
      toaster.success({
        title: 'Permission Fix Started',
        description: 'Please logout and login again for changes to take effect',
        duration: 5000,
      }),
    onError: () =>
      toaster.error({
        title: 'Permission Fix Failed',
        description: 'Failed to fix input group permissions',
        duration: 3000,
      }),
    onSettled: () =>
      queryClient.invalidateQueries({ queryKey: platformStatusQuery.queryKey }),
  })

  return { shortcuts, platformStatus, deleteShortcut, fixPermissions }
}
