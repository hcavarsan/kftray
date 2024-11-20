import React, { useCallback, useState } from 'react'
import { RepeatIcon } from 'lucide-react'

import { Box, Button, Spinner, Text } from '@chakra-ui/react'
import { invoke } from '@tauri-apps/api/tauri'

import { toaster } from '@/components/ui/toaster'
import { Tooltip } from '@/components/ui/tooltip'
import { useGitCredentials } from '@/hooks/useGitCredentials'
import { GitConfig, SyncConfigsButtonProps } from '@/types'

const SyncConfigsButton: React.FC<SyncConfigsButtonProps> = ({
  serviceName,
  accountName,
  onSyncFailure,
  credentialsSaved,
  setCredentialsSaved,
  isGitSyncModalOpen,
}) => {
  const [syncState, setSyncState] = useState({
    lastSync: null as string | null,
    nextSync: null as string | null,
  })

  const handleSyncError = useCallback(
    (error: unknown) => {
      if (!String(error).includes('No matching entry')) {
        onSyncFailure?.(
          error instanceof Error ? error : new Error(String(error)),
        )
        toaster.error({
          title: 'Error syncing configs',
          description: error instanceof Error ? error.message : String(error),
          duration: 200,
        })
      }
    },
    [onSyncFailure],
  )

  const { credentials, isLoading, setIsLoading } = useGitCredentials(
    serviceName,
    accountName,
    isGitSyncModalOpen,
    handleSyncError,
    setCredentialsSaved,
  )

  const syncConfigs = useCallback(
    async (credentials: GitConfig) => {
      if (!credentials) {
        return
      }

      try {
        await invoke('import_configs_from_github', credentials)
        const now = new Date()
        const lastSyncDate = now.toLocaleTimeString()
        const nextSyncDate = new Date(
          now.getTime() + credentials.pollingInterval * 60000,
        ).toLocaleTimeString()

        setSyncState({
          lastSync: lastSyncDate,
          nextSync: nextSyncDate,
        })

        toaster.success({
          title: 'Success',
          description: 'Configs synced successfully',
          duration: 200,
        })
      } catch (error) {
        handleSyncError(error)
      }
    },
    [handleSyncError],
  )

  const handleSyncConfigs = useCallback(async () => {
    if (!credentials) {
      return
    }

    setIsLoading(true)
    await syncConfigs(credentials)
    setIsLoading(false)
  }, [credentials, setIsLoading, syncConfigs])
  const tooltipContent = (
    <Box fontSize='xs' lineHeight='tight'>
      {credentialsSaved ? (
        <>
          <Text>Github Sync Enabled</Text>
          <Text>Repo URL: {credentials?.repoUrl}</Text>
          <Text>Config Path: {credentials?.configPath}</Text>
          <Text>Private Repo: {credentials?.isPrivate ? 'Yes' : 'No'}</Text>
          <Text>Polling Interval: {credentials?.pollingInterval} minutes</Text>
          <Text>Last Sync: {syncState.lastSync ?? ''}</Text>
          <Text>Next Sync: {syncState.nextSync ?? ''}</Text>
        </>
      ) : (
        <Text>Github Sync Disabled</Text>
      )}
    </Box>
  )

  return (
    <Tooltip
      content={tooltipContent}
      portalled
      positioning={{
        strategy: 'absolute',
        placement: 'top-end',
        offset: { mainAxis: 8, crossAxis: 0 },
      }}
    >
      <Button
        size='sm'
        variant='ghost'
        onClick={handleSyncConfigs}
        disabled={!credentialsSaved || isLoading}
        height='26px'
        minWidth='70px'
        bg='whiteAlpha.50'
        px={2}
        borderRadius='md'
        border='1px solid rgba(255, 255, 255, 0.08)'
        _hover={{ bg: 'whiteAlpha.100' }}
        _active={{ bg: 'whiteAlpha.200' }}
      >
        <Box display='flex' alignItems='center' gap={1}>
          {isLoading ? (
            <Spinner size='sm' />
          ) : (
            <Box as={RepeatIcon} width='12px' height='12px' />
          )}
          <Box fontSize='11px'>Sync</Box>
        </Box>
      </Button>
    </Tooltip>
  )
}

export default SyncConfigsButton
