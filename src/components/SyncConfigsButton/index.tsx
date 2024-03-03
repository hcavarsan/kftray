import React, { useEffect, useState } from 'react'
import { FaGithub } from 'react-icons/fa'

import { RepeatIcon } from '@chakra-ui/icons'
import { Box, Button, HStack, Text, Tooltip } from '@chakra-ui/react'
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
  setPollingInterval,
}) => {
  const [isLoading, setIsLoading] = useState(false)
  const [credentials, setCredentials] = useState<GitConfig | null>(null)
  const [lastSync, setLastSync] = useState<string | null>(null)
  const [nextSync, setNextSync] = useState<string | null>(null)

  useEffect(() => {
    async function pollingConfigs() {
      if (credentialsSaved && credentials && credentials.pollingInterval > 0) {
        try {
          const credentialsString = await invoke('get_key', {
            service: serviceName,
            name: accountName,
          })

          if (typeof credentialsString === 'string') {
            const credentials = JSON.parse(credentialsString)

            setPollingInterval(credentials.pollingInterval)

            await invoke('import_configs_from_github', {
              repoUrl: credentials.repoUrl,
              configPath: credentials.configPath,
              isPrivate: credentials.isPrivate,
              pollingInterval: credentials.pollingInterval,
              token: credentials.token,
              flush: true,
            })
            const nextSyncDate = new Date(
              new Date().getTime() + credentials.pollingInterval * 60000,
            ).toLocaleTimeString()
            const lastSyncDate = new Date().toLocaleTimeString()

            setLastSync(lastSyncDate)
            setNextSync(nextSyncDate)
          }
        } catch (error) {
          console.error(
            'Failed to update configs from GitHub during polling:',
            error,
          )
        }
      }
    }

    const pollingId = setInterval(
      () => {
        pollingConfigs()
      },
      credentials && credentials.pollingInterval
        ? credentials.pollingInterval * 60000
        : 0,
    )

    return () => {
      clearInterval(pollingId)
    }
  }, [
    credentialsSaved,
    credentials?.pollingInterval,
    serviceName,
    accountName,
    setPollingInterval,
    credentials,
  ])

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
      if (!credentials) {
        throw new Error('Credentials are not provided')
      }

      await invoke('import_configs_from_github', credentials)

      const nextSyncDate = new Date(
        new Date().getTime() + credentials.pollingInterval * 60000,
      ).toLocaleTimeString()
      const lastSyncDate = new Date().toLocaleTimeString()

      setLastSync(lastSyncDate)
      setNextSync(nextSyncDate)

      onConfigsSynced?.()
    } catch (error) {
      if (error instanceof Error) {
        onSyncFailure?.(error)
      }
    } finally {
      setIsLoading(false)
    }
  }

  const tooltipContent = (
    <Box fontSize='xs' lineHeight='tight'>
      {credentialsSaved ? (
        <>
          <Text>Github Sync Enabled</Text>
          <Text>Repo URL: {credentials?.repoUrl}</Text>
          <Text>Config Path: {credentials?.configPath}</Text>
          <Text>Private Repo: {credentials?.isPrivate ? 'Yes' : 'No'}</Text>
          <Text>Polling Interval: {credentials?.pollingInterval} minutes</Text>
          <Text>Last Sync: {lastSync || ''}</Text>
          <Text>Next Sync: {nextSync || ''}</Text>
        </>
      ) : (
        <Text>Github Sync Disabled</Text>
      )}
    </Box>
  )

  return (
    <Tooltip hasArrow label={tooltipContent} placement='top'>
      <Button
        variant='outline'
        colorScheme='facebook'
        onClick={handleSyncConfigs}
        isDisabled={!credentialsSaved}
        size='sm'
        aria-label='Sync Configs'
        justifyContent='center'
        borderColor='gray.700'
      >
        <HStack spacing={1}>
          <RepeatIcon />
        </HStack>
      </Button>
    </Tooltip>
  )
}

export default SyncConfigsButton
