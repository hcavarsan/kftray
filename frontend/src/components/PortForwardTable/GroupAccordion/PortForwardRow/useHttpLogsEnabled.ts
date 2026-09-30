import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { invoke } from '@tauri-apps/api/core'

import { toaster } from '@/components/ui/toaster'
import { errorMessage } from '@/lib/errors'

export const httpLogsEnabledQueryKey = (configId: number) =>
  ['http-logs-enabled', configId] as const

export function useHttpLogsEnabled(configId: number) {
  return useQuery({
    queryKey: httpLogsEnabledQueryKey(configId),
    queryFn: () => invoke<boolean>('get_http_logs_cmd', { configId }),
  })
}

export function useSetHttpLogsEnabled(configId: number) {
  const queryClient = useQueryClient()

  return useMutation({
    mutationFn: (enable: boolean) =>
      invoke('set_http_logs_cmd', { configId, enable }),
    onSuccess: (_data, enable) => {
      queryClient.setQueryData(httpLogsEnabledQueryKey(configId), enable)
    },
    onError: error => {
      toaster.error({
        title: 'Error toggling HTTP logs',
        description: errorMessage(error),
        duration: 1000,
      })
    },
  })
}
