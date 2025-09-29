import React, { useCallback, useMemo, useRef, useState } from 'react'
import debounce from 'lodash/debounce'
import { RepeatIcon } from 'lucide-react'

import { Box, Button, Spinner, Text } from '@chakra-ui/react'

import { Tooltip } from '@/components/ui/tooltip'
import { useGitSync } from '@/contexts/GitSyncContext'
import { SyncConfigsButtonProps } from '@/types'

const SYNC_DEBOUNCE_MS = 1000
const MAX_RETRIES = 3

const SyncConfigsButton: React.FC<SyncConfigsButtonProps> = ({
  onSyncFailure,
  onSyncComplete,
}) => {
  const { credentials, syncStatus, lastSync, nextSync, syncConfigs } =
    useGitSync()

  const retryCount = useRef(0)
  const [isSyncing, setIsSyncing] = useState(false)

  const handleSync = useCallback(async () => {
    try {
      await syncConfigs()
      retryCount.current = 0
      onSyncComplete?.()
    } catch (error) {
      if (retryCount.current < MAX_RETRIES) {
        retryCount.current++
        await handleSync()
      } else {
        retryCount.current = 0
        onSyncFailure(error instanceof Error ? error : new Error(String(error)))
      }
    }
  }, [syncConfigs, onSyncComplete, onSyncFailure])

  const debouncedSync = useMemo(
    () =>
      debounce(async () => {
        await handleSync()
        setIsSyncing(false)
      }, SYNC_DEBOUNCE_MS),
    [handleSync],
  )

  const handleClick = useCallback(() => {
    if (!isSyncing) {
      setIsSyncing(true)
      debouncedSync()
    }
  }, [debouncedSync, isSyncing])

  const tooltipContent = (
    <Box fontSize='xs' lineHeight='tight'>
      {credentials ? (
        <>
          <Text>Github Sync Enabled</Text>
          <Text>Repo URL: {credentials.repoUrl}</Text>
          <Text>Config Path: {credentials.configPath}</Text>
          <Text>Auth Method: {credentials.authMethod}</Text>
          <Text>Polling Interval: {syncStatus.pollingInterval} minutes</Text>
          <Text>Last Sync: {lastSync ?? ''}</Text>
          <Text>Next Sync: {nextSync ?? ''}</Text>
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
      positioning={{ placement: 'top-start' }}
    >
      <Button
        size='sm'
        variant='ghost'
        onClick={handleClick}
        disabled={!credentials || isSyncing}
        height='32px'
        minWidth='70px'
        bg='whiteAlpha.50'
        px={2}
        borderRadius='md'
        border='1px solid rgba(255, 255, 255, 0.08)'
        _hover={{ bg: 'whiteAlpha.100' }}
        _active={{ bg: 'whiteAlpha.200' }}
      >
        <Box display='flex' alignItems='center' gap={1}>
          {isSyncing ? (
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
