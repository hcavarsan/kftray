import React, { useEffect, useState } from 'react'
import { useQuery } from 'react-query'
import ReactSelect, { ActionMeta, GroupBase, SingleValue } from 'react-select'

import { InfoIcon } from '@chakra-ui/icons'
import {
  Button,
  Center,
  Divider,
  Flex,
  FormControl,
  FormLabel,
  Modal,
  ModalBody,
  ModalContent,
  ModalOverlay,
  Spinner,
  Text,
  Tooltip,
  VStack,
} from '@chakra-ui/react'
import { open } from '@tauri-apps/api/dialog'
import { invoke } from '@tauri-apps/api/tauri'

import { AutoImportModalProps, Config, KubeContext, Option } from '../../types'
import { customStyles, fetchKubeContexts } from '../AddConfigModal/utils'
import useCustomToast from '../CustomToast'

const AutoImportModal: React.FC<AutoImportModalProps> = ({
  isOpen,
  onClose,
}) => {
  const [selectedContext, setSelectedContext] = useState<Option | null>(null)
  const [kubeConfig, setKubeConfig] = useState<string>('default')
  const [isImporting, setIsImporting] = useState(false)
  const toast = useCustomToast()

  const contextQuery = useQuery<KubeContext[]>(
    ['kube-contexts', kubeConfig],
    () => fetchKubeContexts(kubeConfig),
    {
      enabled: isOpen,
      onError: error => {
        console.error('Error fetching contexts:', error)
        if (error instanceof Error) {
          toast({
            title: 'Error fetching contexts',
            description: error.message,
            status: 'error',
          })
        } else {
          toast({
            title: 'Error fetching contexts',
            description: 'An unknown error occurred',
            status: 'error',
          })
        }
      },
    },
  )

  const handleSetKubeConfig = async () => {
    try {
      await invoke('open_save_dialog')
      const selectedPath = await open({
        multiple: false,
        filters: [],
      })

      await invoke('close_save_dialog')

      if (selectedPath) {
        const filePath = Array.isArray(selectedPath)
          ? selectedPath[0]
          : selectedPath

        setKubeConfig(filePath ?? 'default')
      } else {
        console.log('No file selected')
      }
    } catch (error) {
      console.error('Error selecting a file: ', error)
      setKubeConfig('default')
      toast({
        title: 'Error',
        description: 'Failed to select kubeconfig file.',
        status: 'error',
      })
    }
  }

  const handleSelectChange = (
    newValue: SingleValue<Option>,
    { name }: ActionMeta<Option>,
  ) => {
    if (name === 'context') {
      setSelectedContext(newValue)
    }
  }

  const handleImport = async () => {
    if (!selectedContext) {
      toast({
        title: 'Error',
        description: 'Please select a context.',
        status: 'error',
      })

      return
    }

    setIsImporting(true)
    try {
      const configs = await invoke<Config[]>('get_services_with_annotations', {
        contextName: selectedContext.value,
        kubeconfigPath: kubeConfig,
      })

      for (const config of configs) {
        await invoke('insert_config_cmd', { config })
      }

      toast({
        title: 'Success',
        description: 'Configs imported successfully.',
        status: 'success',
      })
      onClose()
    } catch (error) {
      console.error('Failed to import configs:', error)
      toast({
        title: 'Error',
        description: 'Failed to import configs.',
        status: 'error',
      })
    } finally {
      setIsImporting(false)
    }
  }

  useEffect(() => {
    if (!isOpen) {
      setSelectedContext(null)
      setKubeConfig('default')
    }
  }, [isOpen])

  return (
    <Center>
      <Modal isOpen={isOpen} onClose={onClose} size='sm'>
        <ModalOverlay bg='transparent' />
        <ModalContent bg='transparent' borderRadius='20px' marginTop='10'>
          <ModalBody p={0}>
            <form>
              <VStack
                spacing={3}
                align='inherit'
                p={4}
                border='1px'
                borderColor='gray.700'
                borderRadius='20px'
                bg='gray.800'
                boxShadow={`
                  inset 0 2px 6px rgba(0, 0, 0, 0.6),
                  inset 0 -2px 6px rgba(0, 0, 0, 0.6),
                  inset 0 0 0 4px rgba(45, 60, 81, 0.9)
                `}
              >
                <Flex justifyContent='space-between' alignItems='center'>
                  <Text fontSize='xs' fontWeight='bold' color='white'>
                    Auto Import
                  </Text>
                  <Button
                    size='xs'
                    height='26px'
                    width='35%'
                    variant='outline'
                    onClick={handleSetKubeConfig}
                    px={4}
                    borderColor='gray.600'
                  >
                    <Tooltip
                      label={`Current kubeconfig: ${kubeConfig}`}
                      aria-label='Kubeconfig Details'
                      placement='top'
                      hasArrow
                    >
                      <Flex align='center'>
                        <Text fontSize='10px' mr={2}>
                          Set Kubeconfig
                        </Text>
                        <InfoIcon color='gray.500' w={2} h={2} />
                      </Flex>
                    </Tooltip>
                  </Button>
                </Flex>
                <Divider />

                {/* Select Context */}
                <FormControl isRequired>
                  <FormLabel htmlFor='context' fontSize='12px' mb='0'>
                    Context
                  </FormLabel>
                  {contextQuery.isLoading ? (
                    <Flex justifyContent='center' py={2}>
                      <Spinner size='sm' color='white' />
                    </Flex>
                  ) : (
                    <ReactSelect<Option, false, GroupBase<Option>>
                      styles={customStyles}
                      name='context'
                      options={contextQuery.data?.map(context => ({
                        name: context.name,
                        label: context.name,
                        value: context.name,
                      }))}
                      value={selectedContext}
                      onChange={handleSelectChange}
                      isClearable
                      isDisabled={
                        contextQuery.isLoading || contextQuery.isError
                      }
                      placeholder='Select context'
                      theme={theme => ({
                        ...theme,
                        borderRadius: 4,
                        colors: {
                          ...theme.colors,
                          primary25: 'lightgrey',
                          primary: 'grey',
                        },
                      })}
                    />
                  )}
                  {contextQuery.isError && (
                    <Text color='red.500' fontSize='xs'>
                      Error: select a valid kubeconfig
                    </Text>
                  )}
                </FormControl>

                {/* Instructions */}
                <VStack align='start' spacing={2} mt={4} mb={4}>
                  <Text fontSize='xs' color='gray.400'>
                    - Select a context to automatically import services with the
                    annotation{' '}
                    <Text as='span' color='cyan.400' fontWeight='bold'>
                      kftray.app/enabled: true
                    </Text>
                    .
                  </Text>
                  <Text fontSize='xs' color='gray.400'>
                    - If a service has{' '}
                    <Text as='span' color='cyan.400' fontWeight='bold'>
                      kftray.app/configs: &quot;test-9999-http&quot;
                    </Text>
                    , it will use &apos;test&apos; as alias, &apos;9999&apos; as
                    local port, and &apos;http&apos; as target port.
                  </Text>
                </VStack>

                {/* Action Buttons */}
                <Flex justifyContent='flex-end' pt={4} width='100%'>
                  <Button
                    onClick={onClose}
                    size='xs'
                    height='20px'
                    variant='outline'
                    mr={2}
                  >
                    Cancel
                  </Button>
                  <Button
                    height='20px'
                    colorScheme='blue'
                    size='xs'
                    onClick={handleImport}
                    isLoading={isImporting}
                    isDisabled={isImporting || !selectedContext}
                  >
                    Import
                  </Button>
                </Flex>
              </VStack>
            </form>
          </ModalBody>
        </ModalContent>
      </Modal>
    </Center>
  )
}

export default AutoImportModal
