import React, { useCallback, useEffect, useState } from 'react'
import { RepeatIcon } from 'lucide-react'

import { Box, Button, Spinner, Text } from '@chakra-ui/react'
import { invoke } from '@tauri-apps/api/tauri'

import { useCustomToast } from '@/components/ui/toaster'
import { Tooltip } from '@/components/ui/tooltip'
import { GitConfig, SyncConfigsButtonProps } from '@/types'

const SyncConfigsButton: React.FC<SyncConfigsButtonProps> = ({
  serviceName,
  accountName,
  onSyncFailure,
  credentialsSaved,
  setCredentialsSaved,
  isGitSyncModalOpen,
}) => {
  const [state, setState] = useState({
    isLoading: false,
    credentials: null as GitConfig | null,
    lastSync: null as string | null,
    nextSync: null as string | null,
  })
  const toast = useCustomToast()

  const handleSyncError = useCallback((error: unknown) => {
    onSyncFailure?.(error instanceof Error ? error : new Error(String(error)))
    toast({
      title: 'Error syncing configs',
      description: error instanceof Error ? error.message : String(error),
      status: 'error',
    })
  }, [onSyncFailure, toast])

  const syncConfigs = useCallback(async (credentials: GitConfig) => {
    try {
      await invoke('import_configs_from_github', credentials)
      const now = new Date()
      const lastSyncDate = now.toLocaleTimeString()
      const nextSyncDate = new Date(
        now.getTime() + credentials.pollingInterval * 60000,
      ).toLocaleTimeString()

      setState(prev => ({
        ...prev,
        lastSync: lastSyncDate,
        nextSync: nextSyncDate
      }))

      toast({
        title: 'Configs synced successfully',
        status: 'success',
      })
    } catch (error) {
      handleSyncError(error)
    }
  }, [handleSyncError, toast])

  useEffect(() => {
    if (isGitSyncModalOpen) {
      return
    }

    const fetchCredentials = async () => {
      setState(prev => ({ ...prev, isLoading: true }))
      try {
        const credentialsString = await invoke<string>('get_key', {
          service: serviceName,
          name: accountName,
        })
        const credentials = JSON.parse(credentialsString) as GitConfig

        setState(prev => ({ ...prev, credentials }))
        setCredentialsSaved(true)
      } catch (error) {
        setCredentialsSaved(false)
        handleSyncError(error)
      } finally {
        setState(prev => ({ ...prev, isLoading: false }))
      }
    }

    fetchCredentials()
  }, [serviceName, accountName, setCredentialsSaved, isGitSyncModalOpen, handleSyncError])

  const handleSyncConfigs = async () => {
    if (!state.credentials) {
      toast({
        title: 'Error',
        description: 'No credentials available',
        status: 'error',
      })

      return
    }

    setState(prev => ({ ...prev, isLoading: true }))
    await syncConfigs(state.credentials)
    setState(prev => ({ ...prev, isLoading: false }))
  }

  const tooltipContent = (
    <Box fontSize='xs' lineHeight='tight'>
      {credentialsSaved ? (
        <>
          <Text>Github Sync Enabled</Text>
          <Text>Repo URL: {state.credentials?.repoUrl}</Text>
          <Text>Config Path: {state.credentials?.configPath}</Text>
          <Text>Private Repo: {state.credentials?.isPrivate ? 'Yes' : 'No'}</Text>
          <Text>Polling Interval: {state.credentials?.pollingInterval} minutes</Text>
          <Text>Last Sync: {state.lastSync ?? ''}</Text>
          <Text>Next Sync: {state.nextSync ?? ''}</Text>
        </>
      ) : (
        <Text>Github Sync Disabled</Text>
      )}
    </Box>
  )

  return (
    <Tooltip content={tooltipContent}>
      <Button
        size='sm'
        variant='ghost'
        onClick={handleSyncConfigs}
        disabled={!credentialsSaved || state.isLoading}
        className="sync-button"
      >
        <Box display='flex' alignItems='center' gap={1}>
          {state.isLoading ? (
            <Spinner size='xs' />
          ) : (
            <Box as={RepeatIcon} width='13px' height='13px' />
          )}
          <Box fontSize='12px'>Sync</Box>
        </Box>
      </Button>
    </Tooltip>
  )
}

export default SyncConfigsButton
