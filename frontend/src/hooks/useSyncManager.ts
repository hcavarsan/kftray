import { useCallback, useEffect, useState } from 'react'

import { gitService } from '@/services/gitService'
import { SyncStatus } from '@/types'

interface UseSyncManagerProps {
  onSyncFailure: (error: Error) => void
  onSyncComplete: () => void
  credentialsSaved: boolean
}

export function useSyncManager({
  onSyncFailure,
  onSyncComplete,
  credentialsSaved,
}: UseSyncManagerProps) {
  const [syncStatus, setSyncStatus] = useState<SyncStatus>({
    lastSyncTime: null,
    pollingInterval: 0,
    isSuccessful: false,
    isSyncing: false,
  })

  const updateSyncStatus = useCallback((updates: Partial<SyncStatus>) => {
    setSyncStatus(prev => ({
      ...prev,
      ...updates,
    }))
  }, [])

  const syncConfigs = useCallback(async () => {
    updateSyncStatus({ isSyncing: true })

    try {
      const credentials = await gitService.getCredentials(
        'kftray',
        'github_config',
      )

      if (!credentials) {
        throw new Error('No git credentials found')
      }

      await gitService.importConfigs(credentials)
      updateSyncStatus({
        lastSyncTime: Date.now(),
        isSuccessful: true,
        isSyncing: false,
      })
      onSyncComplete()
    } catch (error) {
      updateSyncStatus({
        isSuccessful: false,
        isSyncing: false,
      })
      onSyncFailure(error instanceof Error ? error : new Error('Sync failed'))
    }
  }, [onSyncFailure, onSyncComplete, updateSyncStatus])

  useEffect(() => {
    if (!credentialsSaved || syncStatus.pollingInterval <= 0) {
      return
    }

    const intervalId = setInterval(
      syncConfigs,
      syncStatus.pollingInterval * 60000,
    )

    return () => clearInterval(intervalId)
  }, [credentialsSaved, syncStatus.pollingInterval, syncConfigs])

  return {
    syncStatus,
    syncConfigs,
    updateSyncStatus,
  }
}
