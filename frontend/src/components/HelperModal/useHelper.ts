import { useMutation, useQueryClient } from '@tanstack/react-query'
import { invoke } from '@tauri-apps/api/core'

import { toaster } from '@/components/ui/toaster'

export const HELPER_STATUS_KEY = ['helper-status']

/**
 * Install and uninstall each wait on an elevation prompt and then on the
 * helper's socket, so they can take tens of seconds. Their errors are left
 * on the mutation for the modal to show inline: the backend's message is
 * the only explanation of why elevation or startup failed, too long for a
 * toast.
 */
export function useHelperMutations() {
  const queryClient = useQueryClient()
  const refreshStatus = () =>
    queryClient.invalidateQueries({ queryKey: HELPER_STATUS_KEY })

  const installMutation = useMutation({
    mutationFn: () => invoke<boolean>('install_helper'),
    onSuccess: () => {
      toaster.success({ title: 'kftray-helper installed', duration: 3000 })
    },
    onSettled: refreshStatus,
  })

  const uninstallMutation = useMutation({
    mutationFn: () => invoke<boolean>('remove_helper'),
    onSuccess: () => {
      toaster.success({ title: 'kftray-helper uninstalled', duration: 3000 })
    },
    onSettled: refreshStatus,
  })

  return { installMutation, uninstallMutation, refreshStatus }
}
