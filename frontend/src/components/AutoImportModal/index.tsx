import React, { useEffect, useState } from 'react'
import { useQuery } from 'react-query'
import ReactSelect, { ActionMeta, SingleValue } from 'react-select'

import {
  Button,
  Dialog,
  Flex,
  HStack,
  Spinner,
  Stack,
  Text,
  VStack,
} from '@chakra-ui/react'
import { open } from '@tauri-apps/api/dialog'
import { invoke } from '@tauri-apps/api/tauri'

import { fetchKubeContexts } from '@/components/AddConfigModal/utils'
import { toaster } from '@/components/ui/toaster'
import {
  AutoImportModalProps,
  Config,
  KubeContext,
  StringOption,
} from '@/types'

import { autoImportSelectStyles } from './styles'

const AutoImportModal: React.FC<AutoImportModalProps> = ({
  isOpen,
  onClose,
}) => {
  const [state, setState] = useState({
    selectedContext: null as SingleValue<StringOption>,
    kubeConfig: 'default',
    isImporting: false,
  })

  const contextQuery = useQuery<KubeContext[]>(
    ['kube-contexts', state.kubeConfig],
    () => fetchKubeContexts(state.kubeConfig),
    {
      enabled: isOpen,
      onError: error => {
        console.error('Error fetching contexts:', error)
        toaster.error({
          title: 'Error fetching contexts',
          description:
            error instanceof Error
              ? error.message
              : 'An unknown error occurred',
          duration: 1000,
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
        const filePath = Array.isArray(selectedPath)
          ? selectedPath[0]
          : selectedPath

        setState(prev => ({ ...prev, kubeConfig: filePath ?? 'default' }))
      }
    } catch (error) {
      console.error('Error selecting a file: ', error)
      setState(prev => ({ ...prev, kubeConfig: 'default' }))
      toaster.error({
        title: 'Error',
        description: 'Failed to select kubeconfig file.',
        duration: 1000,
      })
    }
  }

  const handleImport = async () => {
    if (!state.selectedContext) {
      toaster.error({
        title: 'Error',
        description: 'Please select a context.',
        duration: 1000,
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

      toaster.success({
        title: 'Success',
        description: 'Configs imported successfully.',
        duration: 1000,
      })
      onClose()
    } catch (error) {
      console.error('Failed to import configs:', error)
      toaster.error({
        title: 'Error',
        description: 'Failed to import configs.',
        duration: 1000,
      })
    } finally {
      setState(prev => ({ ...prev, isImporting: false }))
    }
  }

  const handleSelectChange = (
    newValue: SingleValue<StringOption>,
    _actionMeta: ActionMeta<StringOption>,
  ) => {
    setState(prev => ({
      ...prev,
      selectedContext: newValue,
    }))
  }

  useEffect(() => {
    if (!isOpen) {
      setState(prev => ({
        ...prev,
        selectedContext: null,
        kubeConfig: 'default',
      }))
    }
  }, [isOpen])

  return (
    <Dialog.Root open={isOpen} onOpenChange={onClose}>
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
          mt={70}
        >
          <Dialog.Header
            p={1.5}
            bg='#161616'
            borderBottom='1px solid rgba(255, 255, 255, 0.05)'
          >
            <Text fontSize='sm' fontWeight='medium' color='gray.100'>
              Auto Import
            </Text>
          </Dialog.Header>

          <Dialog.Body p={3}>
            <Stack gap={4}>
              <Stack gap={1.5}>
                <Flex align='center' justify='space-between'>
                  <Text fontSize='xs' color='gray.400'>
                    Kubeconfig *
                  </Text>
                  <Flex gap={2}>
                    <Button
                      size='xs'
                      variant={
                        state.kubeConfig === 'default' ? 'solid' : 'ghost'
                      }
                      onClick={() =>
                        setState(prev => ({ ...prev, kubeConfig: 'default' }))
                      }
                      bg={
                        state.kubeConfig === 'default'
                          ? 'whiteAlpha.100'
                          : 'transparent'
                      }
                      _hover={{
                        bg:
                          state.kubeConfig === 'default'
                            ? 'whiteAlpha.200'
                            : 'whiteAlpha.50',
                      }}
                      height='22px'
                    >
                      <Text fontSize='xs'>Default</Text>
                    </Button>
                    <Button
                      size='xs'
                      variant={
                        state.kubeConfig !== 'default' ? 'solid' : 'ghost'
                      }
                      onClick={handleSetKubeConfig}
                      bg={
                        state.kubeConfig !== 'default'
                          ? 'whiteAlpha.100'
                          : 'transparent'
                      }
                      _hover={{
                        bg:
                          state.kubeConfig !== 'default'
                            ? 'whiteAlpha.200'
                            : 'whiteAlpha.50',
                      }}
                      height='22px'
                    >
                      <Text fontSize='xs'>Set Custom Kubeconfig</Text>
                    </Button>
                  </Flex>
                </Flex>

                {state.kubeConfig !== 'default' && (
                  <Flex
                    bg='#161616'
                    border='1px solid rgba(255, 255, 255, 0.08)'
                    borderRadius='md'
                    height='35px'
                    align='center'
                    justify='space-between'
                    px={2}
                    _hover={{ borderColor: 'rgba(255, 255, 255, 0.15)' }}
                  >
                    <Text
                      fontSize='xs'
                      color='gray.300'
                      truncate
                      maxW='250px'
                      title={state.kubeConfig}
                    >
                      {state.kubeConfig}
                    </Text>
                    <Button
                      size='xs'
                      variant='ghost'
                      onClick={handleSetKubeConfig}
                      bg='whiteAlpha.50'
                      _hover={{ bg: 'whiteAlpha.100' }}
                      height='22px'
                      minW='70px'
                    >
                      Browse
                    </Button>
                  </Flex>
                )}
                {contextQuery.isError && (
                  <Text color='red.300' fontSize='xs'>
                    Please select a valid kubeconfig file
                  </Text>
                )}
              </Stack>

              <Stack gap={1.5}>
                <Text fontSize='xs' color='gray.400'>
                  Context *
                </Text>
                {contextQuery.isLoading ? (
                  <Flex justify='center' py={2}>
                    <Spinner size='sm' color='blue.400' />
                  </Flex>
                ) : (
                  <ReactSelect<StringOption>
                    options={contextQuery.data?.map(context => ({
                      label: context.name,
                      value: context.name,
                    }))}
                    value={state.selectedContext}
                    onChange={handleSelectChange}
                    styles={autoImportSelectStyles}
                  />
                )}
                {contextQuery.isError && (
                  <Text color='red.300' fontSize='xs'>
                    Please select a valid kubeconfig file
                  </Text>
                )}
              </Stack>

              <VStack align='start' gap={2.5} mt={2}>
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

              <HStack justify='flex-end' gap={2} mt={2}>
                <Button
                  size='xs'
                  variant='ghost'
                  onClick={onClose}
                  _hover={{ bg: 'whiteAlpha.50' }}
                  height='28px'
                >
                  Cancel
                </Button>
                <Button
                  size='xs'
                  bg='blue.500'
                  _hover={{ bg: 'blue.600' }}
                  onClick={handleImport}
                  disabled={!state.selectedContext || state.isImporting}
                  height='28px'
                >
                  Import
                </Button>
              </HStack>
            </Stack>
          </Dialog.Body>
        </Dialog.Content>
      </Dialog.Positioner>
    </Dialog.Root>
  )
}

export default AutoImportModal
