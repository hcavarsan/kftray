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
        toast({
          title: 'Error fetching contexts',
          description:
            error instanceof Error
              ? error.message
              : 'An unknown error occurred',
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
        const filePath = Array.isArray(selectedPath)
          ? selectedPath[0]
          : selectedPath

        setKubeConfig(filePath ?? 'default')
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
    <Dialog.Root open={isOpen} onOpenChange={onClose}>
      <Dialog.Backdrop bg="transparent" />
      <Dialog.Positioner>
        <Dialog.Content
          onClick={(e) => e.stopPropagation()}
          maxWidth="440px"
          width="90vw"
          bg="#161616"
          borderRadius="lg"
          border="1px solid rgba(255, 255, 255, 0.08)"
          overflow="hidden"
        >
          <Dialog.Header
            p={5}
            bg="#161616"
            borderBottom="1px solid rgba(255, 255, 255, 0.05)"
          >
            <Text fontSize="sm" fontWeight="medium" color="gray.100">
              Auto Import
            </Text>
          </Dialog.Header>

          <Dialog.Body p={4}>
            <Stack gap={5}>
              {/* Kubeconfig Selection */}
              <Flex justify="space-between" align="center">
                <Text fontSize="xs" color="gray.400">
                  Kubeconfig
                </Text>
                <Tooltip.Root>
                  <Tooltip.Trigger>
                    <Button
                      size="xs"
                      variant="ghost"
                      onClick={handleSetKubeConfig}
                      bg="whiteAlpha.50"
                      _hover={{ bg: 'whiteAlpha.100' }}
                      height="24px"
                    >
                      <Text fontSize="xs">Set Path</Text>
                    </Button>
                  </Tooltip.Trigger>
                  <Tooltip.Positioner>
                    <Tooltip.Content bg="#161616" border="1px solid rgba(255, 255, 255, 0.05)">
                      <Text fontSize="xs" color="gray.300">{kubeConfig}</Text>
                    </Tooltip.Content>
                  </Tooltip.Positioner>
                </Tooltip.Root>
              </Flex>

              {/* Context Selection */}
              <Stack gap={2}>
                <Text fontSize="xs" color="gray.400">
                  Context
                </Text>
                {contextQuery.isLoading ? (
                  <Flex justify="center" py={2}>
                    <Spinner size="sm" color="blue.400" />
                  </Flex>
                ) : (
                  <ReactSelect
                    options={contextQuery.data?.map(context => ({
                      label: context.name,
                      value: context.name,
                    }))}
                    value={selectedContext}
                    onChange={newValue => setSelectedContext(newValue as Option)}
                    isDisabled={contextQuery.isLoading || contextQuery.isError}
                    placeholder="Select context..."
                    styles={{
                      control: (base) => ({
                        ...base,
                        background: '#161616',
                        borderColor: 'rgba(255, 255, 255, 0.08)',
                        borderRadius: '6px',
                        minHeight: '32px',
                        fontSize: '13px',
                        '&:hover': {
                          borderColor: 'rgba(255, 255, 255, 0.15)'
                        }
                      }),
                      menu: (base) => ({
                        ...base,
                        background: '#161616',
                        border: '1px solid rgba(255, 255, 255, 0.08)',
                        borderRadius: '6px',
                        overflow: 'hidden'
                      }),
                      menuList: (base) => ({
                        ...base,
                        padding: '4px'
                      }),
                      option: (base, state) => ({
                        ...base,
                        fontSize: '13px',
                        background: state.isFocused ? 'rgba(255, 255, 255, 0.05)' : 'transparent',
                        borderRadius: '4px',
                        color: 'rgba(255, 255, 255, 0.9)',
                        padding: '6px 8px',
                        '&:hover': {
                          background: 'rgba(255, 255, 255, 0.05)'
                        }
                      }),
                      singleValue: (base) => ({
                        ...base,
                        color: 'rgba(255, 255, 255, 0.9)',
                        fontSize: '13px'
                      }),
                      input: (base) => ({
                        ...base,
                        color: 'rgba(255, 255, 255, 0.9)',
                        fontSize: '13px'
                      }),
                      placeholder: (base) => ({
                        ...base,
                        color: 'rgba(255, 255, 255, 0.4)',
                        fontSize: '13px'
                      })
                    }}
                  />
                )}
                {contextQuery.isError && (
                  <Text fontSize="xs" color="red.300">
                    Please select a valid kubeconfig file
                  </Text>
                )}
              </Stack>

              {/* Instructions */}
              <VStack
                align="start"
                gap={2.5}
                bg="#161616"
                p={3}
                borderRadius="md"
                border="1px solid rgba(255, 255, 255, 0.05)"
              >
                <Text fontSize="xs" color="gray.300">
                  Services must have:
                </Text>
                <Stack gap={1.5}>
                  <Text fontSize="xs" color="gray.400">
                    • Annotation{' '}
                    <Text as="span" color="blue.300" fontFamily="mono">
                      kftray.app/enabled: true
                    </Text>
                  </Text>
                  <Text fontSize="xs" color="gray.400">
                    • Config format:{' '}
                    <Text as="span" color="blue.300" fontFamily="mono">
                      alias-localPort-targetPort
                    </Text>
                  </Text>
                </Stack>
              </VStack>
            </Stack>
          </Dialog.Body>

          <Dialog.Footer
            p={4}
            borderTop="1px solid rgba(255, 255, 255, 0.05)"
            bg="#161616"
          >
            <HStack gap={3} justify="flex-end">
              <Button
                size="sm"
                variant="ghost"
                onClick={onClose}
                _hover={{ bg: 'whiteAlpha.50' }}
                height="32px"
              >
                Cancel
              </Button>
              <Button
                size="sm"
                bg="blue.500"
                _hover={{ bg: 'blue.600' }}
                onClick={handleImport}
                disabled={isImporting || !selectedContext}
                height="32px"
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
