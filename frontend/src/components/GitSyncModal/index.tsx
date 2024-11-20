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
import { invoke } from '@tauri-apps/api/tauri'

import { Checkbox } from '@/components/ui/checkbox'
import { toaster } from '@/components/ui/toaster'
import { Tooltip } from '@/components/ui/tooltip'
import { GitSyncModalProps } from '@/types'

const GitSyncModal: React.FC<GitSyncModalProps> = ({
  isGitSyncModalOpen,
  closeGitSyncModal,
  credentialsSaved,
  setCredentialsSaved,
  setPollingInterval,
  pollingInterval,
}) => {
  const [settingInputValue, setSettingInputValue] = useState('')
  const [configPath, setConfigPath] = useState('')
  const [isPrivateRepo, setIsPrivateRepo] = useState(false)
  const [gitToken, setGitToken] = useState('')
  const [isLoading, setIsLoading] = useState(false)
  const [isImportAlertOpen, setIsImportAlertOpen] = useState(false)

  const serviceName = 'kftray'
  const accountName = 'github_config'

  useEffect(() => {
    let isComponentMounted = true

    async function getCredentials() {
      if (!isGitSyncModalOpen) {
        return
      }

      setIsLoading(true)
      try {
        const credentialsString = await invoke<string>('get_key', {
          service: serviceName,
          name: accountName,
        })

        if (typeof credentialsString === 'string' && isComponentMounted) {
          const credentials = JSON.parse(credentialsString)

          setSettingInputValue(credentials.repoUrl || '')
          setConfigPath(credentials.configPath || '')
          setIsPrivateRepo(credentials.isPrivate || false)
          setGitToken(credentials.token || '')
          setPollingInterval(credentials.pollingInterval || 60)
          setCredentialsSaved(true)
        }
      } catch (error) {
        console.error('Failed to get git config:', error)
      } finally {
        if (isComponentMounted) {
          setIsLoading(false)
        }
      }
    }

    getCredentials()

    return () => {
      isComponentMounted = false
    }
  }, [isGitSyncModalOpen, setCredentialsSaved, setPollingInterval])

  const handleDeleteGitConfig = async () => {
    setIsLoading(true)
    try {
      await invoke('delete_key', {
        service: serviceName,
        name: accountName,
      })

      setSettingInputValue('')
      setConfigPath('')
      setIsPrivateRepo(false)
      setPollingInterval(0)
      setGitToken('')

      setCredentialsSaved(false)
      closeGitSyncModal()
      toaster.success({
        title: 'Success',
        description: 'Git configuration deleted successfully',
        duration: 200,
      })
    } catch (error) {
      console.error('Failed to delete git config:', error)
      toaster.error({
        title: 'Error deleting git configuration',
        description:
          error instanceof Error ? error.message : 'An unknown error occurred',
        duration: 200,
      })
    } finally {
      setIsLoading(false)
    }
  }

  const handleSaveSettings = async (e: React.FormEvent<HTMLFormElement>) => {
    e.preventDefault()
    setIsImportAlertOpen(true)
  }

  const onConfirmImport = async () => {
    setIsImportAlertOpen(false)
    setIsLoading(true)

    const credentials = {
      repoUrl: settingInputValue,
      configPath: configPath,
      isPrivate: isPrivateRepo,
      token: gitToken,
      pollingInterval: pollingInterval,
      flush: true,
    }

    try {
      await invoke('import_configs_from_github', credentials)
      await invoke('store_key', {
        service: serviceName,
        name: accountName,
        password: JSON.stringify(credentials),
      })

      setCredentialsSaved(true)
      toaster.success({
        title: 'Success',
        description: 'Settings saved successfully',
        duration: 200,
      })
      closeGitSyncModal()
    } catch (error) {
      console.error('Failed to save settings:', error)
      toaster.error({
        title: 'Error saving settings',
        description:
          error instanceof Error ? error.message : 'An unknown error occurred',
        duration: 200,
      })
    } finally {
      setIsLoading(false)
    }
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
            width='90vw'
            maxHeight='95vh'
            height='90vh'
            bg='#111111'
            borderRadius='lg'
            border='1px solid rgba(255, 255, 255, 0.08)'
            overflow='hidden'
            mt={3}
          >
            <Dialog.Header
              p={1.5}
              bg='#161616'
              borderBottom='1px solid rgba(255, 255, 255, 0.05)'
            >
              <Text fontSize='sm' fontWeight='medium' color='gray.100'>
                Configure Github Sync
              </Text>
            </Dialog.Header>

            <Dialog.Body p={3} position='relative' height='calc(100% - 45px)'>
              <form onSubmit={handleSaveSettings}>
                <Stack gap={5} height='100%'>
                  <Stack gap={2}>
                    <Text fontSize='xs' color='gray.400'>
                      GitHub Repository URL
                    </Text>
                    <Input
                      value={settingInputValue}
                      onChange={e => setSettingInputValue(e.target.value)}
                      bg='#161616'
                      borderColor='rgba(255, 255, 255, 0.08)'
                      _hover={{ borderColor: 'rgba(255, 255, 255, 0.15)' }}
                      height='32px'
                      fontSize='13px'
                    />
                  </Stack>

                  <Stack gap={2}>
                    <Text fontSize='xs' color='gray.400'>
                      Config Path
                    </Text>
                    <Input
                      value={configPath}
                      onChange={e => setConfigPath(e.target.value)}
                      bg='#161616'
                      borderColor='rgba(255, 255, 255, 0.08)'
                      _hover={{ borderColor: 'rgba(255, 255, 255, 0.15)' }}
                      height='32px'
                      fontSize='13px'
                    />
                  </Stack>

                  <Stack gap={2}>
                    <Checkbox
                      checked={isPrivateRepo}
                      onCheckedChange={(e: {
                        checked: boolean | 'indeterminate'
                      }) => {
                        const isCheckedBoolean =
                          e.checked === 'indeterminate' ? false : e.checked

                        setIsPrivateRepo(isCheckedBoolean)
                      }}
                    >
                      <Text fontSize='xs' color='gray.400'>
                        Private repository
                      </Text>
                    </Checkbox>

                    <Input
                      type='password'
                      value={gitToken}
                      onChange={e => setGitToken(e.target.value)}
                      placeholder='Git Token'
                      bg='#161616'
                      borderColor='rgba(255, 255, 255, 0.08)'
                      _hover={{ borderColor: 'rgba(255, 255, 255, 0.15)' }}
                      height='32px'
                      fontSize='13px'
                      disabled={!isPrivateRepo}
                      opacity={isPrivateRepo ? 1 : 0.5}
                    />
                  </Stack>

                  <Stack gap={2}>
                    <Text fontSize='xs' color='gray.400'>
                      Polling Interval (minutes)
                    </Text>
                    <Box width='100%' position='relative'>
                      <Tooltip
                        content={`${pollingInterval} minutes`}
                        open={true}
                        showArrow
                      >
                        <Box>
                          <Slider.Root
                            value={[pollingInterval]}
                            min={0}
                            max={120}
                            step={5}
                            onValueChange={(details: { value: number[] }) =>
                              setPollingInterval(details.value[0])
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
                      </Tooltip>
                      <Flex justify='space-between' mt={2}>
                        <Text fontSize='xs' color='gray.400'>
                          0 min
                        </Text>
                        <Text fontSize='xs' color='gray.400'>
                          120 min
                        </Text>
                      </Flex>
                    </Box>
                  </Stack>

                  <Dialog.Footer
                    position='absolute'
                    bottom={0}
                    right={0}
                    left={0}
                    p={3}
                    borderTop='1px solid rgba(255, 255, 255, 0.05)'
                    bg='#111111'
                  >
                    <Flex justify='space-between' width='100%'>
                      <Box>
                        {credentialsSaved && (
                          <Button
                            size='xs'
                            variant='ghost'
                            onClick={handleDeleteGitConfig}
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
                          size='xs'
                          bg='blue.500'
                          _hover={{ bg: 'blue.600' }}
                          disabled={
                            isLoading || !settingInputValue || !configPath
                          }
                          height='28px'
                        >
                          Save Settings
                        </Button>
                      </HStack>
                    </Flex>
                  </Dialog.Footer>
                </Stack>
              </form>
            </Dialog.Body>
          </Dialog.Content>
        </Dialog.Positioner>
      </Dialog.Root>

      <Dialog.Root
        open={isImportAlertOpen}
        onOpenChange={() => setIsImportAlertOpen(false)}
        role='alertdialog'
      >
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
            width='90vw'
            bg='#111111'
            borderRadius='lg'
            border='1px solid rgba(255, 255, 255, 0.08)'
            overflow='hidden'
            mt={150}
          >
            <Dialog.Header
              p={1.5}
              bg='#161616'
              borderBottom='1px solid rgba(255, 255, 255, 0.05)'
            >
              <Text fontSize='sm' fontWeight='medium' color='gray.100'>
                Enable Git Sync
              </Text>
            </Dialog.Header>

            <Dialog.Body p={3}>
              <Text fontSize='xs' color='gray.400'>
                Enabling Git Sync will replace all current configurations with
                those from the git repository. Do you want to continue?
              </Text>
            </Dialog.Body>

            <Dialog.Footer
              p={3}
              borderTop='1px solid rgba(255, 255, 255, 0.05)'
              bg='#111111'
            >
              <HStack justify='flex-end' gap={2}>
                <Button
                  size='xs'
                  variant='ghost'
                  onClick={() => setIsImportAlertOpen(false)}
                  _hover={{ bg: 'whiteAlpha.50' }}
                  height='28px'
                >
                  Cancel
                </Button>
                <Button
                  size='xs'
                  bg='blue.500'
                  _hover={{ bg: 'blue.600' }}
                  onClick={onConfirmImport}
                  height='28px'
                >
                  Import
                </Button>
              </HStack>
            </Dialog.Footer>
          </Dialog.Content>
        </Dialog.Positioner>
      </Dialog.Root>
    </>
  )
}

export default GitSyncModal
