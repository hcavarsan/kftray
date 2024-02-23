/* eslint-disable max-len */
import React, { useEffect, useState } from 'react'

import {
  AlertDialog,
  AlertDialogBody,
  AlertDialogContent,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogOverlay,
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
}) => {
  const [settingInputValue, setSettingInputValue] = useState('')
  const [configPath, setConfigPath] = useState('')
  const [isPrivateRepo, setIsPrivateRepo] = useState(false)
  const [gitToken, setGitToken] = useState('')
  const [isLoading, setIsLoading] = useState(false)
  const [isImportAlertOpen, setIsImportAlertOpen] = useState(false)
  const cancelRef = React.useRef<HTMLButtonElement>(null)

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
          console.log('Credentials found:', credentialsString)
          const credentials = JSON.parse(credentialsString)

          console.log('credentialsString', credentialsString)

          setSettingInputValue(credentials.repoUrl || '')
          setConfigPath(credentials.configPath || '')
          setIsPrivateRepo(credentials.isPrivate || false)
          setGitToken(credentials.token || '')
          setCredentialsSaved(true)
        }
      } catch (error) {
        console.error('Failed to check saved credentials settingsmodal:', error)
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
  }, [isSettingsModalOpen, credentialsSaved, setCredentialsSaved])

  const handleDeleteGitConfig = async () => {
    setIsLoading(true)
    try {
      await invoke('delete_key', {
        service: serviceName,
        name: accountName,
      })
      console.log('Git config deleted')

      setSettingInputValue('')
      setConfigPath('')
      setIsPrivateRepo(false)
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

  const handleCancel = () => {
    closeSettingsModal()
  }

  const handleSaveSettings = async (e: React.FormEvent<HTMLFormElement>) => {
    e.preventDefault()
    setIsImportAlertOpen(true)
  }
  const onConfirmImport = async () => {
    // Close the AlertDialog
    setIsImportAlertOpen(false)

    // Set loading state on
    setIsLoading(true)

    const credentials = JSON.stringify({
      repoUrl: settingInputValue,
      configPath: configPath,
      isPrivate: isPrivateRepo,
      token: gitToken,
      flush: true,
    })

    try {
      await invoke('import_configs_from_github', {
        repoUrl: settingInputValue,
        configPath: configPath,
        isPrivate: isPrivateRepo,
        token: gitToken,
        flush: true,
      })
      await invoke('store_key', {
        service: serviceName,
        name: accountName,
        password: credentials,
      })

      console.log('Credentials saved')
      onSettingsSaved?.()
      setCredentialsSaved(true)
    } catch (error) {
      console.error('Failed to save settings:', error)
    } finally {
      // Set loading state off and close the modal
      setIsLoading(false)
      closeSettingsModal()
    }
  }

  return (
    <Center>
      <Modal isOpen={isSettingsModalOpen} onClose={handleCancel} size='xl'>
        <ModalOverlay bg='transparent' />
        <ModalContent mx={5} my={5} mt={8}>
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

              <FormControl
                p={2}
                display='flex'
                flexDirection='column'
                isDisabled={isLoading}
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
