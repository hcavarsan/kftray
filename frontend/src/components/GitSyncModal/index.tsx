import type { FormEvent } from 'react'
import { useState } from 'react'

import { Box, Button, HStack, Stack, Text } from '@chakra-ui/react'

import { Checkbox } from '@/components/ui/checkbox'
import {
  AppDialog,
  AppDialogBody,
  AppDialogFooter,
  DialogCancelButton,
} from '@/components/ui/dialog'
import { toaster } from '@/components/ui/toaster'
import { useGitSync } from '@/contexts/GitSyncContext'
import { errorMessage } from '@/lib/errors'
import type { GitConfig } from '@/services/gitService'
import type { AuthMethod } from '@/types'

import { AuthFields } from './AuthFields'
import { ConfigPathsField } from './ConfigPathsField'
import { PollingIntervalField } from './PollingIntervalField'
import { RepositoryFields } from './RepositoryFields'

interface GitSyncModalProps {
  onClose: () => void
}

const AUTH_METHODS: AuthMethod[] = ['none', 'system', 'token']

const isAuthMethod = (value: string): value is AuthMethod =>
  AUTH_METHODS.some(method => method === value)

function GitSyncModal({ onClose }: GitSyncModalProps) {
  const { isLoadingCredentials } = useGitSync()

  if (isLoadingCredentials) {
    return null
  }

  return <GitSyncForm onClose={onClose} />
}

function GitSyncForm({ onClose }: GitSyncModalProps) {
  const { credentials, isSaving, saveCredentials, deleteCredentials } =
    useGitSync()

  const [formState, setFormState] = useState(() => ({
    repoUrl: credentials?.repoUrl || '',
    configPaths:
      credentials?.configPaths && credentials.configPaths.length > 0
        ? credentials.configPaths.map(value => ({
            id: crypto.randomUUID(),
            value,
          }))
        : [{ id: crypto.randomUUID(), value: '' }],
    authMethod: credentials?.authMethod ?? 'none',
    gitToken: credentials?.token || '',
    pollingInterval: credentials?.pollingInterval ?? 60,
    flushBeforeSync: credentials?.flush ?? false,
  }))

  const handleConfigPathChange = (id: string, value: string) => {
    setFormState(prev => ({
      ...prev,
      configPaths: prev.configPaths.map(field =>
        field.id === id ? { ...field, value } : field,
      ),
    }))
  }

  const handleAddConfigPath = () => {
    setFormState(prev => ({
      ...prev,
      configPaths: [
        ...prev.configPaths,
        { id: crypto.randomUUID(), value: '' },
      ],
    }))
  }

  const handleRemoveConfigPath = (id: string) => {
    setFormState(prev => ({
      ...prev,
      configPaths:
        prev.configPaths.length > 1
          ? prev.configPaths.filter(field => field.id !== id)
          : [{ id: crypto.randomUUID(), value: '' }],
    }))
  }

  const handleSaveSettings = async (e: FormEvent<HTMLFormElement>) => {
    e.preventDefault()

    const configPaths = formState.configPaths
      .map(field => field.value.trim())
      .filter(value => value.length > 0)

    const newCredentials: GitConfig = {
      repoUrl: formState.repoUrl,
      configPaths,
      authMethod: formState.authMethod,
      token: formState.authMethod === 'token' ? formState.gitToken : '',
      pollingInterval: formState.pollingInterval,
      flush: formState.flushBeforeSync,
    }

    try {
      await saveCredentials(newCredentials)

      toaster.success({
        title: 'Success',
        description:
          'Configurations imported and credentials saved successfully',
        duration: 2000,
      })

      onClose()
    } catch (error) {
      toaster.error({
        title: 'Error saving settings',
        description: errorMessage(error),
        duration: 1000,
      })
    }
  }

  const handleDeleteConfig = async () => {
    try {
      await deleteCredentials()
      onClose()
    } catch (error) {
      toaster.error({
        title: 'Error saving settings',
        description: errorMessage(error),
        duration: 1000,
      })
    }
  }

  const handleAuthMethodChange = (details: { value: string | null }) => {
    const value = details.value

    if (!value || !isAuthMethod(value)) {
      return
    }

    setFormState(prev => ({
      ...prev,
      authMethod: value,
      gitToken: value === 'token' ? prev.gitToken : '',
    }))
  }

  return (
    <AppDialog
      title='Configure Github Sync'
      onClose={onClose}
      maxWidth='400px'
      height='95vh'
    >
      <AppDialogBody>
        <form onSubmit={handleSaveSettings} id='git-sync-form'>
          <Stack gap={4}>
            <RepositoryFields
              repoUrl={formState.repoUrl}
              onRepoUrlChange={repoUrl =>
                setFormState(prev => ({ ...prev, repoUrl }))
              }
            />

            <ConfigPathsField
              configPaths={formState.configPaths}
              onChange={handleConfigPathChange}
              onAdd={handleAddConfigPath}
              onRemove={handleRemoveConfigPath}
            />

            <AuthFields
              authMethod={formState.authMethod}
              gitToken={formState.gitToken}
              onAuthMethodChange={handleAuthMethodChange}
              onGitTokenChange={gitToken =>
                setFormState(prev => ({ ...prev, gitToken }))
              }
            />

            <Stack gap={1}>
              <Checkbox
                checked={formState.flushBeforeSync}
                onCheckedChange={e =>
                  setFormState(prev => ({
                    ...prev,
                    flushBeforeSync: e.checked === true,
                  }))
                }
                size='xs'
              >
                <Text fontSize='xs' color='gray.400'>
                  Flush existing configs before sync
                </Text>
              </Checkbox>
              <Text fontSize='10px' color='gray.500' ml={5} lineHeight='1.3'>
                When enabled, all local configs will be deleted before importing
                from GitHub
              </Text>
            </Stack>

            <PollingIntervalField
              value={formState.pollingInterval}
              onChange={pollingInterval =>
                setFormState(prev => ({ ...prev, pollingInterval }))
              }
            />
          </Stack>
        </form>
      </AppDialogBody>

      <AppDialogFooter justify='space-between'>
        <Box>
          {credentials && (
            <Button
              size='xs'
              variant='ghost'
              onClick={handleDeleteConfig}
              color='red.300'
              _hover={{ bg: 'whiteAlpha.50' }}
              height='28px'
              disabled={isSaving}
            >
              Disable Git Sync
            </Button>
          )}
        </Box>
        <HStack gap={2}>
          <DialogCancelButton onClick={onClose} />
          <Button
            type='submit'
            form='git-sync-form'
            size='xs'
            bg='blue.500'
            _hover={{ bg: 'blue.600' }}
            disabled={
              isSaving ||
              !formState.repoUrl ||
              !formState.configPaths.some(field => field.value.trim()) ||
              (formState.authMethod === 'token' && !formState.gitToken)
            }
            height='28px'
          >
            Save Settings
          </Button>
        </HStack>
      </AppDialogFooter>
    </AppDialog>
  )
}

export default GitSyncModal
