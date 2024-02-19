import React, { useState } from 'react'

import {
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
}) => {
  const [settingInputValue, setSettingInputValue] = useState('')
  const [configPath, setConfigPath] = useState('')
  const [isPrivateRepo, setIsPrivateRepo] = useState(false)
  const [gitToken, setGitToken] = useState('')
  const [isLoading, setIsLoading] = useState(false)
  const [flushConfigs, setFlushConfigs] = useState(false)

  const handleInputChange = (e: React.ChangeEvent<HTMLInputElement>) =>
    setSettingInputValue(e.target.value)
  const handleConfigPathChange = (e: React.ChangeEvent<HTMLInputElement>) =>
    setConfigPath(e.target.value)
  const handleCheckboxChange = (e: React.ChangeEvent<HTMLInputElement>) =>
    setIsPrivateRepo(e.target.checked)
  const handleGitTokenChange = (e: React.ChangeEvent<HTMLInputElement>) =>
    setGitToken(e.target.value)

  const handleFlushConfigsChange = (e: React.ChangeEvent<HTMLInputElement>) =>
    setFlushConfigs(e.target.checked) // New event handler

  const handleSaveSettings = async (e: React.FormEvent<HTMLFormElement>) => {
    e.preventDefault()
    setIsLoading(true)
    try {
      new URL(settingInputValue)
      await invoke('import_configs_from_github', {
        repoUrl: settingInputValue,
        configPath: configPath,
        isPrivate: isPrivateRepo,
        token: isPrivateRepo ? gitToken : undefined,
        flush: flushConfigs,
      })

      setSettingInputValue('')
      setConfigPath('')
      setIsPrivateRepo(false)
      setGitToken('')

      if (typeof onSettingsSaved === 'function') {
        onSettingsSaved()
      }
      closeSettingsModal()
    } catch (e) {
      if (e instanceof TypeError) {
        console.error('Invalid URL:', settingInputValue)
      } else {
        console.error('Error importing configs:', e)
      }
    } finally {
      setIsLoading(false)
    }
  }

  const handleCancel = () => {
    closeSettingsModal()
  }

  return (
    <Center>
      <Modal isOpen={isSettingsModalOpen} onClose={handleCancel} size='xl'>
        <ModalOverlay bg='transparent' />
        <ModalContent mx={5} my={5} mt={8}>
          <ModalCloseButton />
          <ModalBody p={2} mt={3}>
            <form onSubmit={handleSaveSettings}>
              <FormControl p={2} isDisabled={isLoading}>
                <FormLabel htmlFor='settingInput'>
                  GitHub Repository URL
                </FormLabel>
                <Input
                  id='settingInput'
                  type='text'
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
                  id='flushConfigs'
                  isChecked={flushConfigs}
                  onChange={handleFlushConfigsChange}
                  mb={2}
                  mt={2}
                >
                  Flush all existing configs
                </Checkbox>
                <Checkbox
                  id='isPrivateRepo'
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
                <Button
                  variant='outline'
                  onClick={handleCancel}
                  size='xs'
                  isDisabled={isLoading}
                >
                  Cancel
                </Button>
                <Button
                  type='submit'
                  colorScheme='blue'
                  size='xs'
                  ml={3}
                  isLoading={isLoading}
                  isDisabled={isLoading}
                >
                  Save Settings
                </Button>
              </ModalFooter>
            </form>
          </ModalBody>
        </ModalContent>
      </Modal>
    </Center>
  )
}

export default SettingsModal
