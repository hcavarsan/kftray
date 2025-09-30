import React, { useEffect, useState } from 'react'

import {
  Box,
  Button,
  Dialog,
  Flex,
  HStack,
  Input,
  Slider,
  Stack,
  Text,
} from '@chakra-ui/react'

import { Checkbox } from '@/components/ui/checkbox'
import { Radio, RadioGroup } from '@/components/ui/radio'
import { toaster } from '@/components/ui/toaster'
import { useGitSync } from '@/contexts/GitSyncContext'
import { gitService } from '@/services/gitService'
import { GitSyncModalProps } from '@/types'

type AuthMethod = 'none' | 'system' | 'token'

const GitSyncModal: React.FC<GitSyncModalProps> = ({
  isGitSyncModalOpen,
  closeGitSyncModal,
}) => {
  const {
    credentials,
    isLoading,
    saveCredentials,
    deleteCredentials,
    updatePollingInterval,
    syncStatus,
  } = useGitSync()

  const [formState, setFormState] = useState(() => ({
    repoUrl: credentials?.repoUrl || '',
    configPath: credentials?.configPath || '',
    authMethod: (credentials?.authMethod || 'none') as AuthMethod,
    gitToken: credentials?.token || '',
    pollingInterval: syncStatus.pollingInterval || 60,
    flushBeforeSync: credentials?.flush ?? false,
  }))

  useEffect(() => {
    if (isGitSyncModalOpen && credentials) {
      setFormState(prev => ({
        ...prev,
        repoUrl: credentials.repoUrl,
        configPath: credentials.configPath,
        authMethod: credentials.authMethod,
        gitToken: credentials.token || '',
        flushBeforeSync: credentials.flush ?? false,
      }))
    }
  }, [isGitSyncModalOpen, credentials])

  const handlePollingIntervalChange = (value: number) => {
    setFormState(prev => ({ ...prev, pollingInterval: value }))
    updatePollingInterval(value)
  }
  const handleSaveSettings = async (e: React.FormEvent<HTMLFormElement>) => {
    e.preventDefault()

    try {
      const newCredentials = {
        repoUrl: formState.repoUrl,
        configPath: formState.configPath,
        authMethod: formState.authMethod,
        token: formState.authMethod === 'token' ? formState.gitToken : '',
        pollingInterval: formState.pollingInterval,
        flush: formState.flushBeforeSync,
      }

      await gitService.importConfigs(newCredentials)

      await saveCredentials(newCredentials)

      toaster.success({
        title: 'Success',
        description:
          'Configurations imported and credentials saved successfully',
        duration: 2000,
      })

      closeGitSyncModal()
    } catch (error) {
      handleError(error)
    }
  }

  const handleDeleteConfig = async () => {
    try {
      await deleteCredentials()
      closeGitSyncModal()
    } catch (error) {
      handleError(error)
    }
  }

  const handleAuthMethodChange = (event: React.FormEvent<HTMLDivElement>) => {
    const value = (event.target as HTMLInputElement).value as AuthMethod

    setFormState(prev => ({ ...prev, authMethod: value }))
    if (value !== 'token') {
      setFormState(prev => ({ ...prev, gitToken: '' }))
    }
  }

  const handleError = (error: unknown) => {
    console.error('Failed to save settings:', error)
    toaster.error({
      title: 'Error saving settings',
      description:
        error instanceof Error ? error.message : 'An unknown error occurred',
      duration: 1000,
    })
  }

  return (
    <>
      <Dialog.Root open={isGitSyncModalOpen} onOpenChange={closeGitSyncModal}>
        <Dialog.Backdrop
          bg='transparent'
          backdropFilter='blur(4px)'
          borderRadius='lg'
          height='100vh'
        />
        <Dialog.Positioner overflow='hidden'>
          <Dialog.Content
            onClick={e => e.stopPropagation()}
            maxWidth='400px'
            maxHeight='95vh'
            height='100vh'
            bg='#111111'
            borderRadius='lg'
            border='1px solid rgba(255, 255, 255, 0.08)'
            my={2}
            display='flex'
            flexDirection='column'
          >
            <Dialog.Header
              p={1.5}
              bg='#161616'
              borderBottom='1px solid rgba(255, 255, 255, 0.05)'
              flexShrink={0}
            >
              <Text fontSize='sm' fontWeight='medium' color='gray.100'>
                Configure Github Sync
              </Text>
            </Dialog.Header>

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
                  background: 'rgba(255, 255, 255, 0.2)',
                  borderRadius: '3px',
                },
                '&::-webkit-scrollbar-thumb:hover': {
                  background: 'rgba(255, 255, 255, 0.3)',
                },
              }}
            >
              <form onSubmit={handleSaveSettings} id='git-sync-form'>
                <Stack gap={4}>
                  {/* Repository URL */}
                  <Stack gap={2}>
                    <Text fontSize='xs' color='gray.400'>
                      GitHub Repository URL
                    </Text>
                    <Input
                      value={formState.repoUrl}
                      onChange={e =>
                        setFormState(prev => ({
                          ...prev,
                          repoUrl: e.target.value,
                        }))
                      }
                      placeholder='https://github.com/username/repo'
                      bg='#161616'
                      borderColor='rgba(255, 255, 255, 0.08)'
                      position='relative'
                      css={{
                        '&:hover': {
                          borderColor: 'rgba(255, 255, 255, 0.20)',
                          bg: '#161616',
                          zIndex: 2,
                        },
                      }}
                      height='30px'
                      fontSize='12px'
                      borderRadius='md'
                      px={2}
                    />
                  </Stack>

                  {/* Config Path */}
                  <Stack gap={2}>
                    <Text fontSize='xs' color='gray.400'>
                      Config Path
                    </Text>
                    <Input
                      value={formState.configPath}
                      onChange={e =>
                        setFormState(prev => ({
                          ...prev,
                          configPath: e.target.value,
                        }))
                      }
                      placeholder='path/to/config.json'
                      bg='#161616'
                      borderColor='rgba(255, 255, 255, 0.08)'
                      _hover={{
                        borderColor: 'rgba(255, 255, 255, 0.20)',
                        bg: '#161616',
                      }}
                      height='30px'
                      fontSize='12px'
                    />
                  </Stack>

                  {/* Authentication Method */}
                  <Stack gap={2}>
                    <Text fontSize='xs' color='gray.400'>
                      Authentication Method
                    </Text>
                    <Stack
                      direction='row'
                      gap={2}
                      bg='#161616'
                      p={2}
                      borderRadius='md'
                      border='1px solid rgba(255, 255, 255, 0.08)'
                    >
                      <RadioGroup
                        value={formState.authMethod}
                        onChange={handleAuthMethodChange}
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

                    {/* Token Input (only shown when token auth is selected) */}
                    {formState.authMethod === 'token' && (
                      <Input
                        type='password'
                        value={formState.gitToken}
                        onChange={e =>
                          setFormState(prev => ({
                            ...prev,
                            gitToken: e.target.value,
                          }))
                        }
                        placeholder='Enter your GitHub token'
                        bg='#161616'
                        borderColor='rgba(255, 255, 255, 0.08)'
                        _hover={{
                          borderColor: 'rgba(255, 255, 255, 0.20)',
                          bg: '#161616',
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
                    <Text
                      fontSize='10px'
                      color='gray.500'
                      ml={5}
                      lineHeight='1.3'
                    >
                      When enabled, all local configs will be deleted before
                      importing from GitHub
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
                        bg='#161616'
                        borderColor='rgba(255, 255, 255, 0.08)'
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
                          handlePollingIntervalChange(details.value[0])
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
              borderTop='1px solid rgba(255, 255, 255, 0.05)'
              bg='#111111'
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
                      disabled={isLoading}
                    >
                      Disable Git Sync
                    </Button>
                  )}
                </Box>
                <HStack justify='flex-end' gap={2}>
                  <Button
                    size='xs'
                    variant='ghost'
                    onClick={closeGitSyncModal}
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
                      isLoading ||
                      !formState.repoUrl ||
                      !formState.configPath ||
                      (formState.authMethod === 'token' && !formState.gitToken)
                    }
                    height='28px'
                  >
                    Save Settings
                  </Button>
                </HStack>
              </Flex>
            </Dialog.Footer>
          </Dialog.Content>
        </Dialog.Positioner>
      </Dialog.Root>
    </>
  )
}

export default GitSyncModal
