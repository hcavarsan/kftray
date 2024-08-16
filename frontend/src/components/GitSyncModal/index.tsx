/* eslint-disable max-len */
import React, { useEffect, useState } from 'react'
import { MdGraphicEq } from 'react-icons/md'

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
  Divider,
  Flex,
  FormControl,
  FormLabel,
  Input,
  Modal,
  ModalBody,
  ModalContent,
  ModalOverlay,
  Slider,
  SliderFilledTrack,
  SliderThumb,
  SliderTrack,
  Text,
  Tooltip,
  VStack,
} from '@chakra-ui/react'
import { invoke } from '@tauri-apps/api/tauri'

import { GitSyncModalProps } from '../../types'
import useCustomToast from '../CustomToast'

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

  const cancelRef = React.useRef<HTMLButtonElement>(null)
  const [showTooltip, setShowTooltip] = React.useState(false)
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
    serviceName,
    accountName,
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

  return (
    <Center>
      <Modal isOpen={isGitSyncModalOpen} onClose={closeGitSyncModal} size='sm'>
        <ModalOverlay bg='transparent' />
        <ModalContent bg='transparent' borderRadius='20px' marginTop='10'>
          <ModalBody p={0}>
            <form onSubmit={handleSaveSettings}>
              <VStack
                spacing={2}
                align='stretch'
                p={5}
                border='1px'
                borderColor='gray.700'
                borderRadius='20px'
                bg='gray.800'
                boxShadow={`
              /* Inset shadow for top & bottom inner border effect using dark gray */
              inset 0 2px 4px rgba(0, 0, 0, 0.3),
              inset 0 -2px 4px rgba(0, 0, 0, 0.3),
              /* Inset shadow for an inner border all around using dark gray */
              inset 0 0 0 4px rgba(45, 57, 81, 0.9)
            `}
              >
                <Text fontSize='sm' fontWeight='bold'>
                  Configure Github Sync
                </Text>
                <Divider />
                <FormControl mt='4'>
                  <FormLabel htmlFor='settingInput' fontSize='xs'>
                    GitHub Repository URL
                  </FormLabel>
                  <Input
                    id='settingInput'
                    type='text'
                    value={settingInputValue}
                    onChange={handleInputChange}
                    size='xs'
                  />
                </FormControl>
                {/* Continue with other form controls following the same pattern */}
                <FormControl mt='2'>
                  <FormLabel htmlFor='configPath' fontSize='xs'>
                    Config Path
                  </FormLabel>
                  <Input
                    id='configPath'
                    type='text'
                    value={configPath}
                    onChange={handleConfigPathChange}
                    size='xs'
                  />
                </FormControl>
                <FormControl mt='2'>
                  <Checkbox
                    size='sm'
                    isChecked={isPrivateRepo}
                    onChange={handleCheckboxChange}
                  >
                    <Text fontSize='xs'>Private repository</Text>
                  </Checkbox>
                  {isPrivateRepo && (
                    <Input
                      id='gitToken'
                      type='password'
                      value={gitToken}
                      onChange={handleGitTokenChange}
                      placeholder='Git Token'
                      size='xs'
                      mt='2'
                    />
                  )}
                </FormControl>
                {/* Slider for polling interval */}
                <FormControl p={2}>
                  <FormLabel
                    htmlFor='pollingInterval'
                    fontSize='xs'
                    mb='4'
                    mt='8s'
                    ml='-3'
                    color='gray.300'
                    width='100%'
                  >
                    Polling Interval in minutes (set 0 to disable)
                  </FormLabel>
                  <Box position='relative' width='70%'>
                    <Slider
                      id='pollingInterval'
                      defaultValue={pollingInterval}
                      min={0}
                      max={120}
                      step={5}
                      onChange={value => handleSliderChange(value)}
                      onMouseEnter={() => setShowTooltip(true)}
                      onMouseLeave={() => setShowTooltip(false)}
                      colorScheme='facebook'
                    >
                      <SliderTrack bg='gray.700'>
                        <Box position='relative' right={10} />
                        <SliderFilledTrack bg='gray.600' />
                      </SliderTrack>
                      <Tooltip
                        hasArrow
                        bg='gray.600'
                        color='white'
                        placement='top'
                        isOpen={showTooltip}
                        label={`${pollingInterval} min`}
                      >
                        <SliderThumb boxSize={4}>
                          <Box color='gray.600' as={MdGraphicEq} />
                        </SliderThumb>
                      </Tooltip>
                    </Slider>
                    <Flex justifyContent='space-between' mt='2'>
                      <Text fontSize='xs' color='gray.400'>
                        0 min
                      </Text>
                      <Text fontSize='xs' color='gray.400'>
                        120 min
                      </Text>
                    </Flex>
                  </Box>
                </FormControl>
                {/* Buttons */}
                <Flex justifyContent='flex-end' pt={7} width='100%'>
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
                    onClick={closeGitSyncModal}
                    size='xs'
                    mr={2}
                  >
                    Cancel
                  </Button>
                  <Button
                    type='submit'
                    colorScheme='blue'
                    size='xs'
                    isLoading={isLoading}
                    isDisabled={isLoading || !settingInputValue || !configPath}
                  >
                    Save Settings
                  </Button>
                </Flex>
              </VStack>
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

export default GitSyncModal
