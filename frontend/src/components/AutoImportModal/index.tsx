import React, { useEffect, useState } from 'react'
import ReactSelect from 'react-select'

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
  VStack,
} from '@chakra-ui/react'
import { invoke } from '@tauri-apps/api/tauri'

import { AutoImportModalProps, Config, KubeContextInfo } from '../../types'
import { customStyles } from '../AddConfigModal/utils'
import useCustomToast from '../CustomToast'

const AutoImportModal: React.FC<AutoImportModalProps> = ({
  isOpen,
  onClose,
}) => {
  const [contexts, setContexts] = useState<KubeContextInfo[]>([])
  const [selectedContext, setSelectedContext] = useState<{
    label: string
    value: string
  } | null>(null)
  const [isLoading, setIsLoading] = useState(false)
  const [isImporting, setIsImporting] = useState(false)
  const toast = useCustomToast()

  useEffect(() => {
    if (isOpen) {
      setIsLoading(true)
      invoke<KubeContextInfo[]>('list_kube_contexts')
      .then(setContexts)
      .finally(() => setIsLoading(false))
    }
  }, [isOpen])

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

  return (
    <Center>
      <Modal isOpen={isOpen} onClose={onClose} size='sm'>
        <ModalOverlay bg='transparent' />
        <ModalContent borderRadius='20px' marginTop='10'>
          <ModalBody p={0}>
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
              <Text fontSize='lg' fontWeight='bold' color='white'>
                Auto Import
              </Text>
              <Divider borderColor='gray.600' />
              <FormControl mt='4'>
                <FormLabel
                  htmlFor='contextSelect'
                  fontSize='sm'
                  color='gray.300'
                >
                  {isLoading ? 'Loading contexts...' : 'Select Context'}
                </FormLabel>
                {isLoading ? (
                  <Center mt={4} mb={4}>
                    <Spinner size='md' color='white' />
                  </Center>
                ) : (
                  <ReactSelect
                    id='contextSelect'
                    styles={customStyles}
                    placeholder='Select context'
                    options={contexts.map(context => ({
                      label: context.name,
                      value: context.name,
                    }))}
                    value={selectedContext}
                    onChange={setSelectedContext}
                  />
                )}
              </FormControl>
              <VStack align='start' spacing={4} mt={4} mb={4}>
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
              <Flex justifyContent='flex-end' pt={7} width='100%'>
                <Button
                  variant='outline'
                  onClick={onClose}
                  size='xs'
                  mr={2}
                  colorScheme='gray'
                >
                  Cancel
                </Button>
                <Button
                  colorScheme='blue'
                  onClick={handleImport}
                  size='xs'
                  isLoading={isImporting}
                  isDisabled={isImporting || !selectedContext}
                >
                  Import
                </Button>
              </Flex>
            </VStack>
          </ModalBody>
        </ModalContent>
      </Modal>
    </Center>
  )
}

export default AutoImportModal
