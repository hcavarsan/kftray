import React, { useEffect, useState } from 'react'
import { MdGraphicEq } from 'react-icons/md'

import {
  Box,
  Button,
  Checkbox,
  Dialog,
  Flex,
  HStack,
  Input,
  Slider,
  Stack,
  Text,
  Tooltip,
} from '@chakra-ui/react'
import { invoke } from '@tauri-apps/api/tauri'

import { useCustomToast } from '@/components/ui/toaster'
import { GitSyncModalProps } from '@/types'

type ValueChangeDetails = {
  value: number[]
}
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
  const toast = useCustomToast()

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
        const credentialsString = await invoke('get_key', {
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
  }, [
    isGitSyncModalOpen,
    credentialsSaved,
    setCredentialsSaved,
    setPollingInterval,
  ])

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

      setCredentialsSaved(true)
      closeGitSyncModal()
      toast({
        title: 'Git configuration deleted successfully',
        status: 'success',
      })
    } catch (error) {
      console.error('Failed to delete git config:', error)
      toast({
        title: 'Error deleting git configuration',
        description:
          error instanceof Error ? error.message : 'An unknown error occurred',
        status: 'error',
      })
    } finally {
      closeGitSyncModal()
    }
  }

  const handleSaveSettings = async (e: React.FormEvent<HTMLFormElement>) => {
    e.preventDefault()
    setIsImportAlertOpen(true)
  }

  const onConfirmImport = async () => {
    setIsImportAlertOpen(false)
    setIsLoading(true)

    const credentials = JSON.stringify({
      repoUrl: settingInputValue,
      configPath: configPath,
      isPrivate: isPrivateRepo,
      token: gitToken,
      pollingInterval: pollingInterval,
      flush: true,
    })

    try {
      await invoke('import_configs_from_github', {
        repoUrl: settingInputValue,
        configPath: configPath,
        isPrivate: isPrivateRepo,
        token: gitToken,
        pollingInterval: pollingInterval,
        flush: true,
      })
      await invoke('store_key', {
        service: serviceName,
        name: accountName,
        password: credentials,
      })

      setCredentialsSaved(true)
      toast({
        title: 'Settings saved successfully',
        status: 'success',
      })
    } catch (error) {
      console.error('Failed to save settings:', error)
      toast({
        title: 'Error saving settings',
        description:
          error instanceof Error ? error.message : 'An unknown error occurred',
        status: 'error',
      })
    } finally {
      setIsLoading(false)
      closeGitSyncModal()
    }
  }

  const handleSliderChange = (value: ValueChangeDetails) => {
    const numericValue = value.value[0]

    setPollingInterval(numericValue)
  }

  return (
    <>
      <Dialog.Root open={isGitSyncModalOpen} onOpenChange={closeGitSyncModal}>
        <Dialog.Backdrop bg='transparent' />
        <Dialog.Positioner>
          <Dialog.Content
            onClick={e => e.stopPropagation()}
            maxWidth='440px'
            width='90vw'
            bg='#161616'
            borderRadius='lg'
            border='1px solid rgba(255, 255, 255, 0.08)'
            overflow='hidden'
          >
            <Dialog.Header
              p={5}
              bg='#161616'
              borderBottom='1px solid rgba(255, 255, 255, 0.05)'
            >
              <Text fontSize='sm' fontWeight='medium' color='gray.100'>
                Configure Github Sync
              </Text>
            </Dialog.Header>

            <Dialog.Body p={4}>
              <form onSubmit={handleSaveSettings}>
                <Stack gap={5}>
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
                    <Checkbox.Root
                      checked={isPrivateRepo}
                      onCheckedChange={checked => setIsPrivateRepo(!!checked)}
                    >
                      <Checkbox.Control>
                        <Checkbox.Indicator />
                      </Checkbox.Control>
                      <Checkbox.Label>
                        <Text fontSize='xs' color='gray.400'>
                          Private repository
                        </Text>
                      </Checkbox.Label>
                    </Checkbox.Root>
                    {isPrivateRepo && (
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
                      />
                    )}
                  </Stack>

                  <Stack gap={2}>
                    <Text fontSize='xs' color='gray.400'>
                      Polling Interval (minutes)
                    </Text>
                    <Box width='100%'>
                      <Slider.Root
                        value={[pollingInterval]}
                        min={0}
                        max={120}
                        step={5}
                        onValueChange={value => handleSliderChange(value)}
                      >
                        <Slider.Control>
                          <Slider.Track>
                            <Slider.Range />
                          </Slider.Track>
                          <Tooltip.Root>
                            <Tooltip.Trigger asChild>
                              <Slider.Thumb index={0}>
                                <Box as={MdGraphicEq} />
                              </Slider.Thumb>
                            </Tooltip.Trigger>
                            <Tooltip.Positioner>
                              <Tooltip.Content
                                bg='#161616'
                                border='1px solid rgba(255, 255, 255, 0.05)'
                              >
                                <Text fontSize='xs'>{pollingInterval} min</Text>
                              </Tooltip.Content>
                            </Tooltip.Positioner>
                          </Tooltip.Root>
                        </Slider.Control>
                      </Slider.Root>
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
                </Stack>
              </form>
            </Dialog.Body>

            <Dialog.Footer
              p={4}
              borderTop='1px solid rgba(255, 255, 255, 0.05)'
              bg='#161616'
            >
              <HStack gap={3} justify='flex-end'>
                {credentialsSaved && (
                  <Button
                    size='sm'
                    variant='ghost'
                    onClick={handleDeleteGitConfig}
                    color='red.300'
                    _hover={{ bg: 'whiteAlpha.50' }}
                    height='32px'
                    disabled={isLoading}
                  >
                    Disable Git Sync
                  </Button>
                )}
                <Button
                  size='sm'
                  variant='ghost'
                  onClick={closeGitSyncModal}
                  _hover={{ bg: 'whiteAlpha.50' }}
                  height='32px'
                >
                  Cancel
                </Button>
                <Button
                  type='submit'
                  size='sm'
                  bg='blue.500'
                  _hover={{ bg: 'blue.600' }}
                  disabled={isLoading || !settingInputValue || !configPath}
                  height='32px'
                >
                  Save Settings
                </Button>
              </HStack>
            </Dialog.Footer>
          </Dialog.Content>
        </Dialog.Positioner>
      </Dialog.Root>

      <Dialog.Root
        open={isImportAlertOpen}
        onOpenChange={() => setIsImportAlertOpen(false)}
      >
        <Dialog.Backdrop />
        <Dialog.Content>
          <Dialog.Header>
            <Dialog.Title>Enable Git Sync</Dialog.Title>
          </Dialog.Header>
          <Dialog.Body>
            Enabling Git Sync will replace all current configurations with those
            from the git repository. Do you want to continue?
          </Dialog.Body>
          <Dialog.Footer>
            <Button
              variant='outline'
              onClick={() => setIsImportAlertOpen(false)}
            >
              Cancel
            </Button>
            <Button colorPalette='whiteAlpha' onClick={onConfirmImport} ml='3'>
              Import
            </Button>
          </Dialog.Footer>
        </Dialog.Content>
      </Dialog.Root>
    </>
  )
}

export default GitSyncModal
