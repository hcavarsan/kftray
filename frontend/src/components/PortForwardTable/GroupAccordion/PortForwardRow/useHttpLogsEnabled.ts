import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'

import { invoke } from '@/lib/tauri'

export const allHttpLogsEnabledQueryKey = ['http-logs-enabled'] as const

export const httpLogsEnabledQueryKey = (configId: number) =>
  [...allHttpLogsEnabledQueryKey, configId] as const

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
    meta: {
      errorToast: { title: 'Error toggling HTTP logs', duration: 1000 },
    },
  })
}
