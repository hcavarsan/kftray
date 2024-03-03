/* eslint-disable max-len */
import React, { useEffect, useState } from 'react'

import {
  AlertDialog,
  AlertDialogBody,
  AlertDialogContent,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogOverlay,
  Box,
  Button,
  Center,
  Checkbox,
  FormControl,
  FormLabel,
  Input,
  Modal,
  ModalBody,
  ModalCloseButton,
  ModalContent,
  ModalFooter,
  ModalOverlay,
  Slider,
  SliderFilledTrack,
  SliderMark,
  SliderThumb,
  SliderTrack,
  Tooltip,
} from '@chakra-ui/react'
import { invoke } from '@tauri-apps/api/tauri'

import theme from '../../assets/theme'
import { SettingsModalProps } from '../../types'

const SettingsModal: React.FC<SettingsModalProps> = ({
  isSettingsModalOpen,
  closeSettingsModal,
  onSettingsSaved,
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

  const cancelRef = React.useRef<HTMLButtonElement>(null)
  const [showTooltip, setShowTooltip] = React.useState(false)

  const serviceName = 'kftray'
  const accountName = 'github_config'

  useEffect(() => {
    let isComponentMounted = true

    async function getCredentials() {
      if (!isSettingsModalOpen) {
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
        if (isComponentMounted) {
          setCredentialsSaved(false)
        }
      } finally {
        if (isComponentMounted) {
          setIsLoading(false)
        }
      }
    }

    if (isSettingsModalOpen) {
      getCredentials()
    }

    return () => {
      isComponentMounted = false
    }
  }, [
    isSettingsModalOpen,
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

      onSettingsSaved?.()
      setCredentialsSaved(true)
      closeSettingsModal()
    } catch (error) {
      console.error('Failed to delete git config:', error)
    } finally {
      closeSettingsModal()
    }
  }

  const handleInputChange = (e: React.ChangeEvent<HTMLInputElement>) =>
    setSettingInputValue(e.target.value)
  const handleConfigPathChange = (e: React.ChangeEvent<HTMLInputElement>) =>
    setConfigPath(e.target.value)
  const handleCheckboxChange = (e: React.ChangeEvent<HTMLInputElement>) =>
    setIsPrivateRepo(e.target.checked)
  const handleGitTokenChange = (e: React.ChangeEvent<HTMLInputElement>) =>
    setGitToken(e.target.value)

  const handleSliderChange = (value: number) => {
    setPollingInterval(value)
  }

  const handleCancel = () => {
    closeSettingsModal()
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

      onSettingsSaved?.()
      setCredentialsSaved(true)
    } catch (error) {
      console.error('Failed to save settings:', error)
    } finally {
      setIsLoading(false)
      closeSettingsModal()
    }
  }

  return (
    <Center>
      <Modal isOpen={isSettingsModalOpen} onClose={handleCancel} size='xs'>
        <ModalOverlay bg='transparent' />
        <ModalContent
          mx={5}
          my={5}
          mt={8}
          borderRadius='lg'
          boxShadow='0px 10px 25px 5px rgba(0,0,0,0.5)'
        >
          <ModalCloseButton />
          <ModalBody p={2} mt={3}>
            <form onSubmit={handleSaveSettings}>
              <FormControl p={2}>
                <FormLabel htmlFor='settingInput'>
                  GitHub Repository URL
                </FormLabel>
                <Input
                  id='settingInput'
                  type='text'
                  isDisabled={isLoading}
                  value={settingInputValue}
                  onChange={handleInputChange}
                  placeholder='GitHub Repository URL'
                  size='sm'
                  height='36px'
                  bg={theme.colors.gray[800]}
                  borderColor={theme.colors.gray[700]}
                  _hover={{ borderColor: theme.colors.gray[600] }}
                  _placeholder={{ color: theme.colors.gray[500] }}
                  color={theme.colors.gray[300]}
                />
              </FormControl>

              <FormControl p={2} isDisabled={isLoading}>
                <FormLabel htmlFor='configPath'>Config Path</FormLabel>
                <Input
                  id='configPath'
                  type='text'
                  value={configPath}
                  isDisabled={isLoading}
                  onChange={handleConfigPathChange}
                  placeholder='Path to Config File'
                  size='sm'
                  height='36px'
                  bg={theme.colors.gray[800]}
                  borderColor={theme.colors.gray[700]}
                  _hover={{ borderColor: theme.colors.gray[600] }}
                  _placeholder={{ color: theme.colors.gray[500] }}
                  color={theme.colors.gray[300]}
                />
              </FormControl>
              <FormControl p={2} mt='1'>
                <FormLabel htmlFor='pollingInterval'>
                  Polling Interval in minutes (set 0 to disable)
                </FormLabel>
                <Slider
                  id='pollingInterval'
                  value={pollingInterval}
                  min={0}
                  step={5}
                  max={120}
                  colorScheme='facebook'
                  variant='outline'
                  onChange={value => handleSliderChange(value)}
                  onMouseEnter={() => setShowTooltip(true)}
                  onMouseLeave={() => setShowTooltip(false)}
                  width='80%'
                  mx='3'
                  ml='2'
                >
                  <SliderMark value={20} mt='1' ml='-2.5' fontSize='sm'>
                    20
                  </SliderMark>
                  <SliderMark value={60} mt='1' ml='-2.5' fontSize='sm'>
                    60
                  </SliderMark>
                  <SliderMark value={100} mt='1' ml='-2.5' fontSize='sm'>
                    100
                  </SliderMark>
                  <SliderTrack>
                    <SliderFilledTrack />
                  </SliderTrack>
                  <Tooltip
                    hasArrow
                    bg='gray.600'
                    color='white'
                    placement='top'
                    isOpen={showTooltip}
                    label={`${pollingInterval}`}
                  >
                    <SliderThumb />
                  </Tooltip>
                </Slider>
              </FormControl>
              <FormControl
                p={2}
                display='flex'
                flexDirection='column'
                isDisabled={isLoading}
                mt='3'
              >
                <Checkbox
                  id='isPrivateRepo'
                  isDisabled={isLoading}
                  isChecked={isPrivateRepo}
                  onChange={handleCheckboxChange}
                >
                  Private repository
                </Checkbox>
              </FormControl>

              {isPrivateRepo && (
                <FormControl p={2} isDisabled={isLoading}>
                  <FormLabel htmlFor='gitToken'>Git Token</FormLabel>
                  <Input
                    id='gitToken'
                    type='password'
                    value={gitToken}
                    onChange={handleGitTokenChange}
                    isDisabled={isLoading}
                    placeholder='Git Token'
                    size='sm'
                    height='36px'
                    bg={theme.colors.gray[800]}
                    borderColor={theme.colors.gray[700]}
                    _hover={{ borderColor: theme.colors.gray[600] }}
                    _placeholder={{ color: theme.colors.gray[500] }}
                    color={theme.colors.gray[300]}
                  />
                </FormControl>
              )}

              <ModalFooter justifyContent='flex-end' p={2} mt={5}>
                {credentialsSaved && (
                  <Button
                    onClick={handleDeleteGitConfig}
                    variant='outline'
                    colorScheme='red'
                    size='xs'
                    isLoading={isLoading}
                    mr={3}
                  >
                    Disable Git Sync
                  </Button>
                )}
                <Button
                  variant='outline'
                  onClick={handleCancel}
                  disabled={credentialsSaved}
                  size='xs'
                  isDisabled={isLoading}
                  isLoading={isLoading}
                >
                  Cancel
                </Button>
                <Button
                  type='submit'
                  colorScheme='blue'
                  size='xs'
                  ml={3}
                  isLoading={isLoading}
                  isDisabled={isLoading || !settingInputValue || !configPath}
                >
                  Save Settings
                </Button>
              </ModalFooter>
            </form>
          </ModalBody>
        </ModalContent>
      </Modal>
      <AlertDialog
        isOpen={isImportAlertOpen}
        leastDestructiveRef={cancelRef}
        onClose={() => setIsImportAlertOpen(false)}
      >
        <AlertDialogOverlay bg='transparent'>
          <AlertDialogContent>
            <AlertDialogHeader fontSize='xs' fontWeight='bold'>
              Enable Git Sync
            </AlertDialogHeader>

            <AlertDialogBody>
              Enabling Git Sync will replace all current configurations with
              those from the git repository. Do you want to continue?
            </AlertDialogBody>

            <AlertDialogFooter>
              <Button
                ref={cancelRef}
                onClick={() => setIsImportAlertOpen(false)}
              >
                Cancel
              </Button>
              <Button colorScheme='blue' onClick={onConfirmImport} ml={3}>
                Import
              </Button>
            </AlertDialogFooter>
          </AlertDialogContent>
        </AlertDialogOverlay>
      </AlertDialog>
    </Center>
  )
}

export default SettingsModal
