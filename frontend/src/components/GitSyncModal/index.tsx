import type { ChangeEvent, FormEvent } from 'react'
import { useState } from 'react'
import { Plus, X } from 'lucide-react'

import {
  Box,
  Button,
  Dialog,
  Flex,
  HStack,
  IconButton,
  Input,
  Slider,
  Stack,
  Text,
} from '@chakra-ui/react'

import { Checkbox } from '@/components/ui/checkbox'
import { AppDialog } from '@/components/ui/dialog'
import { Radio, RadioGroup } from '@/components/ui/radio'
import { toaster } from '@/components/ui/toaster'
import { useGitSync } from '@/contexts/GitSyncContext'
import { errorMessage } from '@/lib/errors'
import type { GitConfig } from '@/services/gitService'
import type { AuthMethod } from '@/types'

interface GitSyncModalProps {
  onClose: () => void
}

const AUTH_METHODS: AuthMethod[] = ['none', 'system', 'token']

const isAuthMethod = (value: string): value is AuthMethod =>
  AUTH_METHODS.includes(value as AuthMethod)

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
      headerPadding={1.5}
      closable={false}
    >
      <Box
        flex='1'
        overflowY='auto'
        p={3}
        css={{
          '&::-webkit-scrollbar': {
            width: '6px',
          },
          '&::-webkit-scrollbar-track': {
            background: 'transparent',
          },
          '&::-webkit-scrollbar-thumb': {
            background: 'var(--chakra-colors-app-divider)',
            borderRadius: '3px',
          },
          '&::-webkit-scrollbar-thumb:hover': {
            background:
              'color-mix(in srgb, var(--chakra-colors-white) 30%, transparent)',
          },
        }}
      >
        <form onSubmit={handleSaveSettings} id='git-sync-form'>
          <Stack gap={4}>
            <Stack gap={2}>
              <Text fontSize='xs' color='gray.400'>
                GitHub Repository URL
              </Text>
              <Input
                value={formState.repoUrl}
                onChange={(e: ChangeEvent<HTMLInputElement>) =>
                  setFormState(prev => ({
                    ...prev,
                    repoUrl: e.target.value,
                  }))
                }
                placeholder='https://github.com/username/repo'
                bg='app.panel'
                borderColor='app.border'
                position='relative'
                _hover={{
                  borderColor: 'app.divider',
                  bg: 'app.panel',
                  zIndex: 2,
                }}
                height='30px'
                fontSize='12px'
                borderRadius='md'
                px={2}
              />
            </Stack>

            <Stack gap={2}>
              <Flex justify='space-between' align='center'>
                <Text fontSize='xs' color='gray.400'>
                  Config Path(s)
                </Text>
                <IconButton
                  aria-label='Add config path'
                  size='xs'
                  variant='ghost'
                  onClick={handleAddConfigPath}
                  color='gray.400'
                  _hover={{ bg: 'whiteAlpha.100' }}
                >
                  <Box as={Plus} width='12px' height='12px' />
                </IconButton>
              </Flex>
              {formState.configPaths.map(field => (
                <HStack key={field.id} gap={1}>
                  <Input
                    value={field.value}
                    onChange={(e: ChangeEvent<HTMLInputElement>) =>
                      handleConfigPathChange(field.id, e.target.value)
                    }
                    placeholder='path/to/config.json'
                    bg='app.panel'
                    borderColor='app.border'
                    _hover={{
                      borderColor: 'app.divider',
                      bg: 'app.panel',
                    }}
                    height='30px'
                    fontSize='12px'
                  />
                  <IconButton
                    aria-label='Remove config path'
                    size='xs'
                    variant='ghost'
                    onClick={() => handleRemoveConfigPath(field.id)}
                    color='gray.500'
                    _hover={{ bg: 'whiteAlpha.100' }}
                    disabled={
                      formState.configPaths.length === 1 && !field.value
                    }
                  >
                    <Box as={X} width='12px' height='12px' />
                  </IconButton>
                </HStack>
              ))}
            </Stack>

            <Stack gap={2}>
              <Text fontSize='xs' color='gray.400'>
                Authentication Method
              </Text>
              <Stack
                direction='row'
                gap={2}
                bg='app.panel'
                p={2}
                borderRadius='md'
                border='1px solid'
                borderColor='app.border'
              >
                <RadioGroup
                  value={formState.authMethod}
                  onValueChange={handleAuthMethodChange}
                  size='xs'
                >
                  <Stack direction='row' gap={2}>
                    <Radio value='none'>
                      <Text fontSize='xs' color='gray.400'>
                        Public Repository
                      </Text>
                    </Radio>
                    <Radio value='system'>
                      <Text fontSize='xs' color='gray.400'>
                        Use System Git Credentials
                      </Text>
                    </Radio>
                    <Radio value='token'>
                      <Text fontSize='xs' color='gray.400'>
                        GitHub Token
                      </Text>
                    </Radio>
                  </Stack>
                </RadioGroup>
              </Stack>

              {formState.authMethod === 'token' && (
                <Input
                  type='password'
                  value={formState.gitToken}
                  onChange={(e: ChangeEvent<HTMLInputElement>) =>
                    setFormState(prev => ({
                      ...prev,
                      gitToken: e.target.value,
                    }))
                  }
                  placeholder='Enter your GitHub token'
                  bg='app.panel'
                  borderColor='app.border'
                  _hover={{
                    borderColor: 'app.divider',
                    bg: 'app.panel',
                  }}
                  height='30px'
                  fontSize='12px'
                />
              )}
            </Stack>

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

            <Stack gap={2} mt={2}>
              <Flex justify='space-between' align='center'>
                <Text fontSize='xs' color='gray.400'>
                  Polling Interval (minutes)
                </Text>
                <Input
                  value={
                    formState.pollingInterval === 0
                      ? 'off'
                      : `${formState.pollingInterval} min`
                  }
                  readOnly
                  width='65px'
                  height='24px'
                  textAlign='center'
                  bg='app.panel'
                  borderColor='app.border'
                  fontSize='11px'
                  _disabled={{
                    opacity: 0.8,
                    cursor: 'default',
                  }}
                />
              </Flex>
              <Box>
                <Slider.Root
                  value={[formState.pollingInterval]}
                  min={0}
                  max={120}
                  step={5}
                  onValueChange={details =>
                    setFormState(prev => ({
                      ...prev,
                      pollingInterval: details.value[0],
                    }))
                  }
                >
                  <Slider.Control>
                    <Slider.Track>
                      <Slider.Range />
                    </Slider.Track>
                    <Slider.Thumb index={0} />
                  </Slider.Control>
                </Slider.Root>
              </Box>
              <Flex justify='space-between' align='center'>
                <Text fontSize='xs' color='gray.400'>
                  Disabled
                </Text>
                <Text fontSize='xs' color='gray.400'>
                  120 min
                </Text>
              </Flex>
            </Stack>
          </Stack>
        </form>
      </Box>

      <Dialog.Footer
        p={3}
        borderTop='1px solid'
        borderColor='app.hover'
        bg='app.bg'
        flexShrink={0}
      >
        <Flex justify='space-between' width='100%'>
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
          <HStack justify='flex-end' gap={2}>
            <Button
              size='xs'
              variant='ghost'
              onClick={onClose}
              _hover={{ bg: 'whiteAlpha.50' }}
              height='28px'
            >
              Cancel
            </Button>
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
        </Flex>
      </Dialog.Footer>
    </AppDialog>
  )
}

export default GitSyncModal
