import React, {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
} from 'react'

import { invoke } from '@tauri-apps/api/tauri'

import { toaster } from '@/components/ui/toaster'
import { GitConfig, SyncStatus } from '@/types'

interface GitSyncContextType {
  credentials: GitConfig | null
  isLoading: boolean
  syncStatus: SyncStatus
  lastSync: string | null
  nextSync: string | null
  saveCredentials: (creds: GitConfig) => Promise<void>
  deleteCredentials: () => Promise<void>
  syncConfigs: () => Promise<void>
  updatePollingInterval: (interval: number) => void
}

const GitSyncContext = createContext<GitSyncContextType | null>(null)

const CREDENTIALS_CACHE_KEY = 'git-sync-credentials'
const SERVICE_NAME = 'kftray'
const ACCOUNT_NAME = 'github_config'

export const GitSyncProvider: React.FC<{ children: React.ReactNode }> = ({
  children,
}) => {
  const [credentials, setCredentials] = useState<GitConfig | null>(null)
  const [isLoading, setIsLoading] = useState(false)
  const [syncStatus, setSyncStatus] = useState<SyncStatus>({
    lastSyncTime: null,
    isSuccessful: false,
    pollingInterval: 60,
    isSyncing: false,
  })

  const cachedCredentials = useMemo(() => credentials, [credentials])

  useEffect(() => {
    const loadCredentials = async () => {
      try {
        const cached = sessionStorage.getItem(CREDENTIALS_CACHE_KEY)

        if (cached) {
          setCredentials(JSON.parse(cached))

          return
        }

        const credentialsString = await invoke<string>('get_key', {
          service: SERVICE_NAME,
          name: ACCOUNT_NAME,
        })

        if (credentialsString) {
          const creds = JSON.parse(credentialsString)

          setCredentials(creds)
          sessionStorage.setItem(CREDENTIALS_CACHE_KEY, credentialsString)
        }
      } catch (error) {
        console.error('Failed to fetch credentials:', error)
      }
    }

    loadCredentials()
  }, [])

  const saveCredentials = useCallback(async (newCredentials: GitConfig) => {
    setIsLoading(true)
    try {
      await invoke('store_key', {
        service: SERVICE_NAME,
        name: ACCOUNT_NAME,
        password: JSON.stringify(newCredentials),
      })
      setCredentials(newCredentials)
      sessionStorage.setItem(
        CREDENTIALS_CACHE_KEY,
        JSON.stringify(newCredentials),
      )
    } catch (error) {
      throw error
    } finally {
      setIsLoading(false)
    }
  }, [])

  const syncConfigs = useCallback(async () => {
    if (!credentials || syncStatus.isSyncing) {
      return
    }

    setSyncStatus(prev => ({ ...prev, isSyncing: true }))
    try {
      await invoke('import_configs_from_github', {
        repoUrl: credentials.repoUrl,
        configPath: credentials.configPath,
        useSystemCredentials: credentials.authMethod === 'system',
        flush: true,
        githubToken:
          credentials.authMethod === 'token' ? credentials.token : null,
      })

      const now = new Date()

      setSyncStatus(prev => ({
        ...prev,
        lastSyncTime: now.getTime(),
        isSuccessful: true,
        isSyncing: false,
      }))

      toaster.success({
        title: 'Success',
        description: 'Configs synced successfully',
        duration: 1000,
      })
    } catch (error) {
      setSyncStatus(prev => ({
        ...prev,
        isSuccessful: false,
        isSyncing: false,
      }))
      throw error
    }
  }, [credentials, syncStatus.isSyncing])

  const value = useMemo(
    () => ({
      credentials: cachedCredentials,
      isLoading,
      syncStatus,
      lastSync: syncStatus.lastSyncTime
        ? new Date(syncStatus.lastSyncTime).toLocaleTimeString()
        : null,
      nextSync: syncStatus.lastSyncTime
        ? new Date(
          syncStatus.lastSyncTime + syncStatus.pollingInterval * 60000,
        ).toLocaleTimeString()
        : null,
      saveCredentials,
      deleteCredentials: async () => {
        await invoke('delete_key', {
          service: SERVICE_NAME,
          name: ACCOUNT_NAME,
        })
        setCredentials(null)
        sessionStorage.removeItem(CREDENTIALS_CACHE_KEY)
      },
      syncConfigs,
      updatePollingInterval: (interval: number) =>
        setSyncStatus(prev => ({ ...prev, pollingInterval: interval })),
    }),
    [cachedCredentials, isLoading, syncStatus, saveCredentials, syncConfigs],
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
