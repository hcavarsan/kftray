import { RepeatIcon } from 'lucide-react'

import { Box, Spinner, Text } from '@chakra-ui/react'

import { FooterActionButton } from '@/components/Footer/FooterActionButton'
import { toaster } from '@/components/ui/toaster'
import { Tooltip } from '@/components/ui/tooltip'
import { useGitSync } from '@/contexts/GitSyncContext'

function SyncConfigsButton() {
  const { credentials, syncStatus, lastSync, nextSync, syncConfigs } =
    useGitSync()

  const handleClick = () =>
    syncConfigs().then(
      () =>
        toaster.success({
          title: 'Success',
          description: 'Configs synced successfully',
          duration: 1000,
        }),
      () => undefined,
    )

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
    <Tooltip content={tooltipContent} positioning={{ placement: 'top-start' }}>
      <FooterActionButton
        aria-label='Sync configs from git'
        onClick={handleClick}
        disabled={!credentials || syncStatus.isSyncing}
        minWidth='70px'
        px={2}
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
      </FooterActionButton>
    </Tooltip>
  )
}

export default SyncConfigsButton
