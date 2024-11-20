import { useCallback, useEffect, useState } from 'react'

import { invoke } from '@tauri-apps/api/tauri'

import { GitConfig } from '@/types'

interface GitCredentialsState {
  isLoading: boolean
  credentials: GitConfig | null
  lastSync: string | null
  nextSync: string | null
  hasCheckedCredentials: boolean
}

interface GitCredentialsReturn {
  credentials: GitConfig | null
  isLoading: boolean
  setIsLoading: (value: boolean) => void
}

export function useGitCredentials(
  serviceName: string,
  accountName: string,
  isGitSyncModalOpen: boolean,
  onError: (error: unknown) => void,
  setCredentialsSaved: (value: boolean) => void,
): GitCredentialsReturn {
  const [state, setState] = useState<GitCredentialsState>({
    isLoading: false,
    credentials: null,
    lastSync: null,
    nextSync: null,
    hasCheckedCredentials: false,
  })

  const fetchCredentials = useCallback(async () => {
    if (state.hasCheckedCredentials || isGitSyncModalOpen) {
      return
    }

    setState(prev => ({ ...prev, isLoading: true }))

    try {
      const credentialsString = await invoke<string>('get_key', {
        service: serviceName,
        name: accountName,
      })

      const credentials = JSON.parse(credentialsString) as GitConfig

      setState(prev => ({
        ...prev,
        credentials,
        isLoading: false,
        hasCheckedCredentials: true,
      }))
      setCredentialsSaved(true)
    } catch (error) {
      if (
        error instanceof Error &&
        !error.toString().includes('No matching entry')
      ) {
        onError(error)
      }
      setState(prev => ({
        ...prev,
        credentials: null,
        isLoading: false,
        hasCheckedCredentials: true,
      }))
      setCredentialsSaved(false)
    }
  }, [
    serviceName,
    accountName,
    isGitSyncModalOpen,
    onError,
    setCredentialsSaved,
    state.hasCheckedCredentials,
  ])

  useEffect(() => {
    fetchCredentials()
  }, [fetchCredentials])

  const setIsLoading = useCallback((value: boolean) => {
    setState(prev => ({ ...prev, isLoading: value }))
  }, [])

  return {
    credentials: state.credentials,
    isLoading: state.isLoading,
    setIsLoading,
  }
}
