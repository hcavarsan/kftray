import { RepeatIcon } from 'lucide-react'

import { Box, Button, Spinner, Text } from '@chakra-ui/react'

import { toaster } from '@/components/ui/toaster'
import { Tooltip } from '@/components/ui/tooltip'
import { useGitSync } from '@/contexts/GitSyncContext'
import { errorMessage } from '@/lib/errors'

function SyncConfigsButton() {
  const { credentials, syncStatus, lastSync, nextSync, syncConfigs } =
    useGitSync()

  const handleClick = async () => {
    try {
      await syncConfigs()
      toaster.success({
        title: 'Success',
        description: 'Configs synced successfully',
        duration: 1000,
      })
    } catch (error) {
      toaster.error({
        title: 'Sync failed',
        description: errorMessage(error),
        duration: 2000,
      })
    }
  }

  const tooltipContent = (
    <Box fontSize='xs' lineHeight='tight'>
      {credentials ? (
        <>
          <Text>Github Sync Enabled</Text>
          <Text>Repo URL: {credentials.repoUrl}</Text>
          <Text>Config Path(s): {credentials.configPaths.join(', ')}</Text>
          <Text>Auth Method: {credentials.authMethod}</Text>
          <Text>Polling Interval: {credentials.pollingInterval} minutes</Text>
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
        aria-label='Sync configs from git'
        size='sm'
        variant='ghost'
        onClick={handleClick}
        disabled={!credentials || syncStatus.isSyncing}
        height='32px'
        minWidth='70px'
        bg='whiteAlpha.50'
        px={2}
        borderRadius='md'
        border='1px solid'
        borderColor='app.border'
        _hover={{ bg: 'whiteAlpha.100' }}
        _active={{ bg: 'whiteAlpha.200' }}
      >
        <Box display='flex' alignItems='center' gap={1}>
          {syncStatus.isSyncing ? (
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
