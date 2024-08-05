import React, { useEffect, useState } from 'react'

import { RepeatIcon } from '@chakra-ui/icons'
import { Box, Button, HStack, Spinner, Text, Tooltip } from '@chakra-ui/react'
import { invoke } from '@tauri-apps/api/tauri'

import { GitConfig, SyncConfigsButtonProps } from '../../../types'
import useCustomToast from '../../CustomToast'

const SyncConfigsButton: React.FC<SyncConfigsButtonProps> = ({
  serviceName,
  accountName,
  updateConfigsWithState,
  onSyncFailure,
  credentialsSaved,
  setCredentialsSaved,
  isGitSyncModalOpen,
  setPollingInterval,
}) => {
  const [isLoading, setIsLoading] = useState<boolean>(false)
  const [credentials, setCredentials] = useState<GitConfig | null>(null)
  const [lastSync, setLastSync] = useState<string | null>(null)
  const [nextSync, setNextSync] = useState<string | null>(null)
  const toast = useCustomToast()

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
      credentials?.pollingInterval ? credentials.pollingInterval * 60000 : 0,
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
    if (!isGitSyncModalOpen) {
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
    isGitSyncModalOpen,
    setIsLoading,
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

      updateConfigsWithState?.()
      toast({
        title: 'Configs synced successfully',
        status: 'success',
      })
    } catch (error) {
      if (error instanceof Error) {
        onSyncFailure?.(error)
        toast({
          title: 'Error syncing configs',
          description: error.message,
          status: 'error',
        })
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
          <Text>Last Sync: {lastSync ?? ''}</Text>
          <Text>Next Sync: {nextSync ?? ''}</Text>
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
        isDisabled={!credentialsSaved || isLoading}
        size='sm'
        aria-label='Sync Configs'
        justifyContent='center'
        borderColor='gray.700'
      >
        {isLoading ? (
          <HStack spacing={1}>
            <Spinner size='sm' />
          </HStack>
        ) : (
          <HStack spacing={1}>
            <RepeatIcon />
          </HStack>
        )}
      </Button>
    </Tooltip>
  )
}

export default SyncConfigsButton
