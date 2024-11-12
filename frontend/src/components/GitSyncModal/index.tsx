import React, { useEffect, useState } from 'react'
import { MdGraphicEq } from 'react-icons/md'

import {
  Box,
  Button,
  Checkbox,
  Dialog,
  Field,
  Fieldset,
  Flex,
  HStack,
  Input,
  Separator,
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
        <Dialog.Backdrop />
        <Dialog.Content
          maxWidth='sm'
          position='relative'
          bg='gray.800'
          borderRadius='xl'
          boxShadow='xl'
        >
          <Dialog.Body p='0'>
            <form onSubmit={handleSaveSettings}>
              <Fieldset.Root>
                <Stack
                  gap='4'
                  p='4'
                  border='1px'
                  borderColor='gray.700'
                  borderRadius='xl'
                  bg='gray.800'
                  css={{
                    boxShadow: `
                      inset 0 2px 4px rgba(0, 0, 0, 0.3),
                      inset 0 -2px 4px rgba(0, 0, 0, 0.3),
                      inset 0 0 0 4px rgba(45, 57, 81, 0.9)
                    `,
                  }}
                >
                  <Fieldset.Legend>
                    <Text fontSize='sm' fontWeight='semibold'>
                      Configure Github Sync
                    </Text>
                  </Fieldset.Legend>

                  <Separator />

                  <Fieldset.Content>
                    <Stack gap='4'>
                      <Field.Root>
                        <Field.Label>GitHub Repository URL</Field.Label>
                        <Input
                          value={settingInputValue}
                          onChange={e => setSettingInputValue(e.target.value)}
                          size='sm'
                        />
                      </Field.Root>
                      <Field.Root>
                        <Field.Label>Config Path</Field.Label>
                        <Input
                          value={configPath}
                          onChange={e => setConfigPath(e.target.value)}
                          size='sm'
                        />
                      </Field.Root>
                      <Field.Root>
                        <Checkbox.Root
                          checked={isPrivateRepo}
                          onCheckedChange={checked =>
                            setIsPrivateRepo(!!checked)
                          }
                        >
                          <Checkbox.Control>
                            <Checkbox.Indicator />
                          </Checkbox.Control>
                          <Checkbox.Label>
                            <Text fontSize='sm'>Private repository</Text>
                          </Checkbox.Label>
                        </Checkbox.Root>
                        {isPrivateRepo && (
                          <Input
                            type='password'
                            value={gitToken}
                            onChange={e => setGitToken(e.target.value)}
                            placeholder='Git Token'
                            size='sm'
                            mt='2'
                          />
                        )}
                      </Field.Root>
                      <Field.Root>
                        <Field.Label>
                          Polling Interval in minutes (set 0 to disable)
                        </Field.Label>
                        <Box position='relative' width='70%'>
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
                                  <Tooltip.Content>
                                    <Text>{pollingInterval} min</Text>
                                  </Tooltip.Content>
                                </Tooltip.Positioner>
                              </Tooltip.Root>
                            </Slider.Control>
                          </Slider.Root>
                          <Flex justify='space-between' mt='2'>
                            <Text fontSize='xs' color='gray.400'>
                              0 min
                            </Text>
                            <Text fontSize='xs' color='gray.400'>
                              120 min
                            </Text>
                          </Flex>
                        </Box>
                      </Field.Root>
                      s
                      <HStack justify='flex-end' pt='4'>
                        {credentialsSaved && (
                          <Button
                            onClick={handleDeleteGitConfig}
                            variant='outline'
                            colorPalette='red'
                            size='sm'
                            disabled={isLoading}
                          >
                            Disable Git Sync
                          </Button>
                        )}
                        <Button
                          variant='outline'
                          onClick={closeGitSyncModal}
                          size='sm'
                        >
                          Cancel
                        </Button>
                        <Button
                          type='submit'
                          size='sm'
                          disabled={
                            isLoading || !settingInputValue || !configPath
                          }
                        >
                          Save Settings
                        </Button>
                      </HStack>
                    </Stack>
                  </Fieldset.Content>
                </Stack>
              </Fieldset.Root>
            </form>
          </Dialog.Body>
        </Dialog.Content>
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
