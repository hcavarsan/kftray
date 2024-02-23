import React, { useEffect, useState } from 'react'

import { RepeatIcon } from '@chakra-ui/icons'
import { Box, Button, Text, Tooltip } from '@chakra-ui/react'
import { invoke } from '@tauri-apps/api/tauri'

import { GitConfig, SyncConfigsButtonProps } from '../../types'

const SyncConfigsButton: React.FC<SyncConfigsButtonProps> = ({
  serviceName,
  accountName,
  onConfigsSynced,
  onSyncFailure,
  credentialsSaved,
  setCredentialsSaved,
  isSettingsModalOpen,
}) => {
  const [isLoading, setIsLoading] = useState(false)
  const [credentials, setCredentials] = useState<GitConfig | null>(null)

  useEffect(() => {
    if (!isSettingsModalOpen) {
      ;(async () => {
        setIsLoading(true)

        try {
          const credentialsString = await invoke('get_key', {
            service: serviceName,
            name: accountName,
          })

          if (typeof credentialsString === 'string') {
            const creds = JSON.parse(credentialsString)

            setCredentials(creds)
            setCredentialsSaved(true)
          }
        } catch (error) {
          console.error('Failed to check saved credentials syncbutton:', error)
          if (error instanceof Error) {
            onSyncFailure?.(error)
          }
          setCredentialsSaved(false)
        } finally {
          setIsLoading(false)
        }
      })()
    }
  }, [
    serviceName,
    accountName,
    onSyncFailure,
    setCredentialsSaved,
    isSettingsModalOpen,
  ])

  const handleSyncConfigs = async () => {
    setIsLoading(true)
    try {
      await invoke('import_configs_from_github', credentials)
      console.log('Configs synced successfully')
      onConfigsSynced?.()
    } catch (error) {
      console.error('Error syncing configs:', error)
      if (error instanceof Error) {
        onSyncFailure?.(error)
      }
    } finally {
      setIsLoading(false)
    }
  }

  if (!credentialsSaved) {
    return null
  }

  const tooltipContent = (
    <Box fontSize='sm' lineHeight='tight'>
      <Text>Repo URL: {credentials?.repoUrl}</Text>
      <Text>Config Path: {credentials?.configPath}</Text>
      <Text>Private Repo: {credentials?.isPrivate ? 'Yes' : 'No'}</Text>
    </Box>
  )

  return (
    <Tooltip hasArrow label={tooltipContent} placement='top' shouldWrapChildren>
      <Button
        onClick={handleSyncConfigs}
        size='xs'
        colorScheme='facebook'
        disabled={isLoading}
        variant='outline'
        leftIcon={<RepeatIcon />}
      >
        Sync Configs
      </Button>
    </Tooltip>
  )
}

export default SyncConfigsButton
