import React, { useEffect, useState } from 'react'
import { useQuery } from 'react-query'
import ReactSelect from 'react-select'

import {
  Button,
  Dialog,
  Flex,
  HStack,
  Spinner,
  Stack,
  Text,
  Tooltip,
  VStack,
} from '@chakra-ui/react'
import { open } from '@tauri-apps/api/dialog'
import { invoke } from '@tauri-apps/api/tauri'

import { fetchKubeContexts } from '@/components/AddConfigModal/utils'
import { useCustomToast } from '@/components/ui/toaster'
import { AutoImportModalProps, Config, KubeContext, Option } from '@/types'

const AutoImportModal: React.FC<AutoImportModalProps> = ({
  isOpen,
  onClose,
}) => {
  const [state, setState] = useState({
    selectedContext: null as Option | null,
    kubeConfig: 'default',
    isImporting: false,
  })
  const toast = useCustomToast()

  const contextQuery = useQuery<KubeContext[]>(
    ['kube-contexts', state.kubeConfig],
    () => fetchKubeContexts(state.kubeConfig),
    {
      enabled: isOpen,
      onError: error => {
        console.error('Error fetching contexts:', error)
        toast({
          title: 'Error fetching contexts',
          description: error instanceof Error ? error.message : 'An unknown error occurred',
          status: 'error',
        })
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
        const filePath = Array.isArray(selectedPath) ? selectedPath[0] : selectedPath


        setState(prev => ({ ...prev, kubeConfig: filePath ?? 'default' }))
      }
    } catch (error) {
      console.error('Error selecting a file: ', error)
      setState(prev => ({ ...prev, kubeConfig: 'default' }))
      toast({
        title: 'Error',
        description: 'Failed to select kubeconfig file.',
        status: 'error',
      })
    }
  }

  const handleImport = async () => {
    if (!state.selectedContext) {
      toast({
        title: 'Error',
        description: 'Please select a context.',
        status: 'error',
      })

      return
    }

    setState(prev => ({ ...prev, isImporting: true }))
    try {
      const configs = await invoke<Config[]>('get_services_with_annotations', {
        contextName: state.selectedContext.value,
        kubeconfigPath: state.kubeConfig,
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
      setState(prev => ({ ...prev, isImporting: false }))
    }
  }

  useEffect(() => {
    if (!isOpen) {
      setState(prev => ({ ...prev, selectedContext: null, kubeConfig: 'default' }))
    }
  }, [isOpen])

  return (
    <Dialog.Root open={isOpen} onOpenChange={onClose}>
      <Dialog.Backdrop bg='transparent' />
      <Dialog.Positioner>
        <Dialog.Content className="auto-import-dialog">
          <Dialog.Header>
            <Text fontSize='sm' fontWeight='medium' color='gray.100'>
              Auto Import
            </Text>
          </Dialog.Header>

          <Dialog.Body p={4}>
            <Stack gap={5}>
              <Flex justify='space-between' align='center'>
                <Text fontSize='xs' color='gray.400'>
                  Kubeconfig
                </Text>
                <Tooltip.Root>
                  <Tooltip.Trigger>
                    <Button
                      size='xs'
                      variant='ghost'
                      onClick={handleSetKubeConfig}
                      className="kubeconfig-button"
                    >
                      <Text fontSize='xs'>Set Path</Text>
                    </Button>
                  </Tooltip.Trigger>
                  <Tooltip.Content>
                    <Text fontSize='xs' color='gray.300'>
                      {state.kubeConfig}
                    </Text>
                  </Tooltip.Content>
                </Tooltip.Root>
              </Flex>

              <Stack gap={2}>
                <Text fontSize='xs' color='gray.400'>
                  Context
                </Text>
                {contextQuery.isLoading ? (
                  <Flex justify='center' py={2}>
                    <Spinner size='sm' color='blue.400' />
                  </Flex>
                ) : (
                  <ReactSelect
                    options={contextQuery.data?.map(context => ({
                      label: context.name,
                      value: context.name,
                    }))}
                    value={state.selectedContext}
                    onChange={newValue => setState(prev => ({ ...prev, selectedContext: newValue as Option }))}
                    isDisabled={contextQuery.isLoading || contextQuery.isError}
                    placeholder='Select context...'
                    className="context-select"
                  />
                )}
                {contextQuery.isError && (
                  <Text fontSize='xs' color='red.300'>
                    Please select a valid kubeconfig file
                  </Text>
                )}
              </Stack>

              <VStack align='start' gap={2.5} className="instructions">
                <Text fontSize='xs' color='gray.300'>
                  Services must have:
                </Text>
                <Stack gap={1.5}>
                  <Text fontSize='xs' color='gray.400'>
                    • Annotation{' '}
                    <Text as='span' color='blue.300' fontFamily='mono'>
                      kftray.app/enabled: true
                    </Text>
                  </Text>
                  <Text fontSize='xs' color='gray.400'>
                    • Config format:{' '}
                    <Text as='span' color='blue.300' fontFamily='mono'>
                      alias-localPort-targetPort
                    </Text>
                  </Text>
                </Stack>
              </VStack>
            </Stack>
          </Dialog.Body>

          <Dialog.Footer>
            <HStack gap={3} justify='flex-end'>
              <Button
                size='sm'
                variant='ghost'
                onClick={onClose}
                className="cancel-button"
              >
                Cancel
              </Button>
              <Button
                size='sm'
                bg='blue.500'
                onClick={handleImport}
                disabled={state.isImporting || !state.selectedContext}
                className="import-button"
              >
                Import
              </Button>
            </HStack>
          </Dialog.Footer>
        </Dialog.Content>
      </Dialog.Positioner>
    </Dialog.Root>
  )
}

export default AutoImportModal
