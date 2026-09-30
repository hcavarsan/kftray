import type { ReactNode } from 'react'
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
} from 'react'

import {
  type UseMutateFunction,
  useMutation,
  useQuery,
  useQueryClient,
} from '@tanstack/react-query'

import { configsQuery } from '@/hooks/useConfigs'
import type { GitConfig } from '@/services/gitService'
import { gitService } from '@/services/gitService'

interface SyncStatus {
  lastSyncTime: number | null
  isSyncing: boolean
}

interface GitSyncContextValue {
  credentials: GitConfig | null
  isLoadingCredentials: boolean
  isSaving: boolean
  syncStatus: SyncStatus
  lastSync: string | null
  nextSync: string | null
  saveCredentials: UseMutateFunction<GitConfig, Error, GitConfig>
  deleteCredentials: UseMutateFunction<void, Error, void>
  syncConfigs: () => Promise<void>
}

const GitSyncContext = createContext<GitSyncContextValue | null>(null)

const credentialsQueryKey = ['git-sync-credentials']

const importMutationKey = ['git-import']

const importScope = { id: 'git-import' }

export function GitSyncProvider({ children }: { children: ReactNode }) {
  const queryClient = useQueryClient()
  const [lastSyncTime, setLastSyncTime] = useState<number | null>(null)

  const { data: credentials = null, isLoading: isLoadingCredentials } =
    useQuery({
      queryKey: credentialsQueryKey,
      queryFn: () => gitService.getCredentials(),
      staleTime: Number.POSITIVE_INFINITY,
    })

  const { mutateAsync: syncAsync, isPending: isSyncing } = useMutation({
    mutationKey: importMutationKey,
    scope: importScope,
    mutationFn: (creds: GitConfig) => gitService.importConfigs(creds),
    onSuccess: () => {
      setLastSyncTime(Date.now())
      queryClient.invalidateQueries({ queryKey: configsQuery.queryKey })
    },
    meta: { errorToast: { title: 'Sync Failed', duration: 3000 } },
    retry: 2,
    retryDelay: attempt => Math.min(1000 * 2 ** attempt, 8000),
  })

  const saveMutation = useMutation({
    mutationKey: importMutationKey,
    scope: importScope,
    mutationFn: async (creds: GitConfig) => {
      await gitService.importConfigs(creds)
      await gitService.saveCredentials(creds)

      return creds
    },
    onSuccess: creds => {
      queryClient.setQueryData(credentialsQueryKey, creds)
      queryClient.invalidateQueries({ queryKey: configsQuery.queryKey })
      setLastSyncTime(Date.now())
    },
    meta: { errorToast: { title: 'Error saving settings', duration: 1000 } },
  })

  const deleteMutation = useMutation({
    mutationFn: () => gitService.deleteCredentials(),
    onSuccess: () => {
      queryClient.setQueryData(credentialsQueryKey, null)
    },
    meta: { errorToast: { title: 'Error saving settings', duration: 1000 } },
  })

  const syncConfigs = useCallback(async () => {
    if (!credentials) {
      throw new Error('No git credentials found')
    }

    await syncAsync(credentials)
  }, [credentials, syncAsync])

  useEffect(() => {
    const interval = credentials?.pollingInterval ?? 0

    if (!credentials || interval <= 0) {
      return
    }

    const intervalId = setInterval(() => {
      if (queryClient.isMutating({ mutationKey: importMutationKey }) > 0) {
        return
      }
      syncAsync(credentials).catch(() => undefined)
    }, interval * 60000)

    return () => clearInterval(intervalId)
  }, [credentials, syncAsync, queryClient])

  const { mutate: saveCredentials } = saveMutation
  const { mutate: deleteCredentials } = deleteMutation

  const value = useMemo<GitSyncContextValue>(
    () => ({
      credentials,
      isLoadingCredentials,
      isSaving: saveMutation.isPending || deleteMutation.isPending,
      syncStatus: {
        lastSyncTime,
        isSyncing,
      },
      lastSync: lastSyncTime
        ? new Date(lastSyncTime).toLocaleTimeString()
        : null,
      nextSync:
        lastSyncTime && credentials && credentials.pollingInterval > 0
          ? new Date(
              lastSyncTime + credentials.pollingInterval * 60000,
            ).toLocaleTimeString()
          : null,
      saveCredentials,
      deleteCredentials,
      syncConfigs,
    }),
    [
      credentials,
      isLoadingCredentials,
      saveMutation.isPending,
      deleteMutation.isPending,
      isSyncing,
      lastSyncTime,
      saveCredentials,
      deleteCredentials,
      syncConfigs,
    ],
  )

  return (
    <GitSyncContext.Provider value={value}>{children}</GitSyncContext.Provider>
  )
}

export const useGitSync = () => {
  const context = useContext(GitSyncContext)

  if (!context) {
    throw new Error('useGitSync must be used within GitSyncProvider')
  }

  return context
}
