/* eslint-disable complexity */

import React, { useEffect, useMemo, useState } from 'react'
import Select, { ActionMeta, MultiValue, SingleValue } from 'react-select'

import {
  Button,
  Dialog,
  Flex,
  Grid,
  HStack,
  Input,
  Separator,
  Stack,
  Text,
  Tooltip,
} from '@chakra-ui/react'
import { useQuery } from '@tanstack/react-query'
import { open } from '@tauri-apps/api/dialog'
import { invoke } from '@tauri-apps/api/tauri'

import { Checkbox } from '@/components/ui/checkbox'
import { toaster } from '@/components/ui/toaster'
import {
  Config,
  CustomConfigProps,
  PortOption,
  ServiceData,
  StringOption,
} from '@/types'

import { selectStyles } from './styles'
import { trimConfigValues, validateFormFields } from './utils'

const AddConfigModal: React.FC<CustomConfigProps> = ({
  isModalOpen,
  closeModal,
  newConfig,
  handleInputChange,
  handleSaveConfig,
  isEdit,
  setNewConfig,
}) => {
  const workloadTypeOptions: StringOption[] = [
    { value: 'service', label: 'Service' },
    { value: 'pod', label: 'Pod' },
    { value: 'proxy', label: 'Proxy' },
  ]

  const CUSTOM_PORT = {
    value: -1,
    label: 'Custom Port...',
  } as const

  const protocolOptions: StringOption[] = [
    { value: 'tcp', label: 'TCP' },
    { value: 'udp', label: 'UDP' },
  ]

  const [formState, setFormState] = useState({
    selectedContext: null as StringOption | null,
    selectedNamespace: null as StringOption | null,
    selectedServiceOrTarget: null as StringOption | null,
    selectedPort: null as PortOption | null,
    selectedWorkloadType: null as StringOption | null,
    selectedProtocol: null as StringOption | null,
  })

  const [uiState, setUiState] = useState({
    isChecked: false,
    isContextDropdownFocused: false,
    isFormValid: false,
    kubeConfig: 'default',
  })

  const handleError = (error: unknown, title: string) => {
    console.error(`Error: ${title}`, error)
    toaster.error({
      title,
      description:
        error instanceof Error ? error.message : 'An unknown error occurred',
      duration: 1000,
    })
  }

  useEffect(() => {
    const requiredFields = [
      formState.selectedContext?.value ?? null,
      formState.selectedNamespace?.value ?? null,
      formState.selectedServiceOrTarget?.value ??
        newConfig.remote_address ??
        null,
      formState.selectedPort?.value ?? newConfig.remote_port ?? null,
      formState.selectedWorkloadType?.value ?? null,
      formState.selectedProtocol?.value ?? null,
    ]

    setUiState(prev => ({
      ...prev,
      isFormValid: validateFormFields(requiredFields),
    }))
  }, [formState, newConfig])

  const handleCheckboxChange = (
    name: string,
    e: { checked: boolean | 'indeterminate' },
  ) => {
    const isCheckedBoolean = e.checked === 'indeterminate' ? false : e.checked

    if (name === 'domain_enabled') {
      setNewConfig(prev => ({
        ...prev,
        domain_enabled: isCheckedBoolean,
      }))
    } else if (name === 'local_address_enabled') {
      setUiState(prev => ({
        ...prev,
        isChecked: isCheckedBoolean,
      }))
      setNewConfig(prev => ({
        ...prev,
        local_address_enabled: isCheckedBoolean,
      }))
    }
  }

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

        setUiState(prev => ({
          ...prev,
          kubeConfig: filePath ?? 'default',
        }))
      }
    } catch (error) {
      handleError(error, 'Error selecting file')
      setUiState(prev => ({
        ...prev,
        kubeConfig: 'default',
      }))
    }
  }

  const contextQuery = useQuery<{ name: string }[]>({
    queryKey: ['kube-contexts', uiState.kubeConfig],
    queryFn: () =>
      invoke<{ name: string }[]>('list_kube_contexts', {
        kubeconfig: uiState.kubeConfig,
      }),
    enabled:
      isModalOpen &&
      (uiState.isContextDropdownFocused || uiState.kubeConfig !== 'default'),
  })

  // Handle context query errors
  useEffect(() => {
    if (contextQuery.error) {
      handleError(contextQuery.error, 'Error fetching contexts')
    }
  }, [contextQuery.error])

  const namespaceQuery = useQuery<{ name: string }[]>({
    queryKey: ['kube-namespaces', newConfig.context],
    queryFn: () =>
      invoke<{ name: string }[]>('list_namespaces', {
        contextName: newConfig.context,
        kubeconfig: uiState.kubeConfig,
      }),
    enabled: isModalOpen && !!newConfig.context,
  })

  // Handle namespace query errors
  useEffect(() => {
    if (namespaceQuery.error) {
      handleError(namespaceQuery.error, 'Error fetching namespaces')
    }
  }, [namespaceQuery.error])

  const serviceQuery = useQuery<ServiceData[]>({
    queryKey: ['services', newConfig.context, newConfig.namespace],
    queryFn: () =>
      invoke<ServiceData[]>('list_services', {
        contextName: newConfig.context,
        namespace: newConfig.namespace,
        kubeconfig: uiState.kubeConfig,
      }),
    enabled:
      isModalOpen &&
      !!newConfig.context &&
      !!newConfig.namespace &&
      newConfig.workload_type === 'service',
  })

  // Handle service query errors
  useEffect(() => {
    if (serviceQuery.error) {
      handleError(serviceQuery.error, 'Error fetching services')
    }
  }, [serviceQuery.error])

  const podsQuery = useQuery<{ labels_str: string }[]>({
    queryKey: ['kube-pods', newConfig.context, newConfig.namespace],
    queryFn: () =>
      invoke<{ labels_str: string }[]>('list_pods', {
        contextName: newConfig.context,
        namespace: newConfig.namespace,
        kubeconfig: uiState.kubeConfig,
      }),
    enabled:
      isModalOpen &&
      !!newConfig.context &&
      !!newConfig.namespace &&
      newConfig.workload_type === 'pod',
  })

  // Handle pods query errors
  useEffect(() => {
    if (podsQuery.error) {
      handleError(podsQuery.error, 'Error fetching pods')
    }
  }, [podsQuery.error])

  const portQuery = useQuery<{ name: string; port: number }[]>({
    queryKey: [
      'kube-service-ports',
      newConfig.context,
      newConfig.namespace,
      newConfig.workload_type === 'pod' ? newConfig.target : newConfig.service,
    ],
    queryFn: async () => {
      const params = {
        contextName: newConfig.context,
        namespace: newConfig.namespace,
        serviceName:
          newConfig.workload_type === 'pod'
            ? newConfig.target
            : (newConfig.service ?? newConfig.target),
        kubeconfig: uiState.kubeConfig,
      }

      const ports = await invoke<
        { name: string | null; port: string | number | null }[]
      >('list_ports', params)

      console.log(ports)

      return ports
      .filter(p => p.port != null)
      .map(p => ({
        name: p.name ?? '',
        port:
            typeof p.port === 'string'
              ? parseInt(p.port, 10)
              : (p.port as number),
      }))
    },
    enabled:
      isModalOpen &&
      !!newConfig.context &&
      !!newConfig.namespace &&
      !!(newConfig.workload_type === 'pod'
        ? newConfig.target
        : newConfig.service) &&
      newConfig.workload_type !== 'proxy',
  })

  // Handle port query errors
  useEffect(() => {
    if (portQuery.error) {
      handleError(portQuery.error, 'Error fetching service ports')
    }
  }, [portQuery.error])

  const getServiceOrTargetValue = (config: Config) => {
    if (config.service) {
      return { label: config.service, value: config.service }
    }
    if (config.target) {
      return { label: config.target, value: config.target }
    }

    return null
  }

  useEffect(() => {
    if (isEdit && isModalOpen) {
      setFormState(prev => ({
        ...prev,
        selectedWorkloadType: newConfig.workload_type
          ? { label: newConfig.workload_type, value: newConfig.workload_type }
          : null,
        selectedProtocol: newConfig.protocol
          ? { label: newConfig.protocol, value: newConfig.protocol }
          : null,
        selectedContext: newConfig.context
          ? { label: newConfig.context, value: newConfig.context }
          : null,
        selectedNamespace: newConfig.namespace
          ? { label: newConfig.namespace, value: newConfig.namespace }
          : null,
        selectedServiceOrTarget: getServiceOrTargetValue(newConfig),
        selectedPort: newConfig.remote_port
          ? {
            label: newConfig.remote_port.toString(),
            value: newConfig.remote_port,
          }
          : null,
      }))
      setUiState(prev => ({
        ...prev,
        kubeConfig: newConfig.kubeconfig ?? 'default',
      }))
    }
  }, [isEdit, isModalOpen, newConfig])

  useEffect(() => {
    if (!isModalOpen) {
      resetState()
    }
  }, [isModalOpen])

  const resetState = () => {
    setFormState(prev => ({
      ...prev,
      selectedContext: null,
      selectedNamespace: null,
      selectedServiceOrTarget: null,
      selectedPort: null,
      selectedWorkloadType: null,
      selectedProtocol: null,
    }))
    setUiState(prev => ({
      ...prev,
      isChecked: false,
      isContextDropdownFocused: false,
      isFormValid: false,
      kubeConfig: 'default',
    }))
  }

  const handleSelectChange = (
    newValue:
      | SingleValue<StringOption | PortOption>
      | MultiValue<StringOption | PortOption>,
    actionMeta: ActionMeta<StringOption | PortOption>,
  ) => {
    if (actionMeta.name === 'remote_port') {
      const portOption = newValue as PortOption | null

      if (portOption?.value === -1) {
        setFormState(prev => ({
          ...prev,
          selectedPort: portOption,
        }))
        setNewConfig(prev => ({ ...prev, remote_port: undefined }))
      } else {
        setFormState(prev => ({
          ...prev,
          selectedPort: portOption,
        }))
        setNewConfig(prev => ({ ...prev, remote_port: portOption?.value }))
      }

      return
    }
    const option = newValue as StringOption | null

    switch (actionMeta.name) {
    case 'context':
      setFormState(prev => ({
        ...prev,
        selectedContext: option,
      }))
      break
    case 'namespace':
      setFormState(prev => ({
        ...prev,
        selectedNamespace: option,
      }))
      break
    case 'service':
    case 'target':
      setFormState(prev => ({
        ...prev,
        selectedServiceOrTarget: option,
      }))
      break
    case 'workload_type':
      setFormState(prev => ({
        ...prev,
        selectedWorkloadType: option,
      }))
      break
    case 'protocol':
      setFormState(prev => ({
        ...prev,
        selectedProtocol: option,
      }))
      break
    }

    handleInputChange({
      target: {
        name: actionMeta.name,
        value: option ? option.value : '',
      } as HTMLInputElement,
    } as React.ChangeEvent<HTMLInputElement>)
  }

  useEffect(() => {
    if (setNewConfig) {
      setNewConfig(prev => ({
        ...prev,
        kubeconfig: uiState.kubeConfig ?? '',
      }))
    }
  }, [uiState.kubeConfig, setNewConfig])

  const handleSave = async (event: React.FormEvent) => {
    event.preventDefault()
    const configToSave = trimConfigValues(newConfig)

    await handleSaveConfig(configToSave)
    if (!isEdit) {
      resetState()
    }
    closeModal()
  }

  const handleCancel = () => {
    closeModal()
    resetState()
  }

  const portOptions: PortOption[] = useMemo(
    () => [
      ...(portQuery.data?.map(port => ({
        value: port.port,
        label: port.name ? `${port.name} (${port.port})` : port.port.toString(),
      })) || []),
    ],
    [portQuery.data],
  )

  return (
    <Dialog.Root open={isModalOpen} onOpenChange={handleCancel}>
      <Dialog.Backdrop
        bg='transparent'
        backdropFilter='blur(4px)'
        borderRadius='lg'
        maxHeight='100vh'
        overflow='hidden'
      />
      <Dialog.Positioner>
        <Dialog.Content
          onClick={e => e.stopPropagation()}
          maxWidth='600px'
          width='90vw'
          maxHeight='100vh'
          bg='#111111'
          borderRadius='lg'
          border='1px solid rgba(255, 255, 255, 0.08)'
          overflow='hidden'
          position='absolute'
          mt={4}
        >
          {/* Compact Header */}
          <Dialog.Header
            p={3}
            bg='#161616'
            borderBottom='1px solid rgba(255, 255, 255, 0.05)'
          >
            <Flex justify='space-between' align='center'>
              <Text fontSize='sm' fontWeight='medium' color='gray.100'>
                {isEdit ? 'Edit Configuration' : 'Add Configuration'}
              </Text>

              {/* Kubeconfig Section */}
              <HStack gap={2}>
                <Text fontSize='2xs' color='gray.400'>
                  Kubeconfig:
                </Text>
                <Tooltip.Root>
                  <Tooltip.Trigger asChild>
                    <Button
                      size='xs'
                      variant='ghost'
                      onClick={handleSetKubeConfig}
                      bg='whiteAlpha.50'
                      _hover={{ bg: 'whiteAlpha.200' }}
                      height='20px'
                      px={2}
                    >
                      <Text fontSize='2xs' maxW='120px' truncate>
                        {uiState.kubeConfig}
                      </Text>
                    </Button>
                  </Tooltip.Trigger>
                  <Tooltip.Positioner>
                    <Tooltip.Content bg='#ffffff'>
                      <Text fontSize='2xs'>{uiState.kubeConfig}</Text>
                    </Tooltip.Content>
                  </Tooltip.Positioner>
                </Tooltip.Root>
              </HStack>
            </Flex>
          </Dialog.Header>

          <Dialog.Body p={3}>
            <form onSubmit={handleSave}>
              <Stack gap={2}>
                <Grid templateColumns='repeat(2, 1fr)' gap={3}>
                  <Stack gap={1.5}>
                    <Text fontSize='xs' color='gray.400'>
                      Alias
                    </Text>
                    <Input
                      value={newConfig.alias || ''}
                      name='alias'
                      onChange={handleInputChange}
                      bg='#161616'
                      border='1px solid rgba(255, 255, 255, 0.08)'
                      _hover={{ borderColor: 'rgba(255, 255, 255, 0.15)' }}
                      _focus={{ borderColor: 'blue.400', boxShadow: 'none' }}
                      height='28px'
                      fontSize='13px'
                    />
                    <Checkbox
                      size='xs'
                      checked={newConfig.domain_enabled}
                      onCheckedChange={e =>
                        handleCheckboxChange('domain_enabled', e)
                      }
                    >
                      <Text fontSize='xs' color='gray.400'>
                        Enable alias as domain
                      </Text>
                    </Checkbox>
                  </Stack>

                  <Stack gap={1.5}>
                    <Text fontSize='xs' color='gray.400'>
                      Context *
                    </Text>
                    <Select
                      name='context'
                      value={formState.selectedContext}
                      onChange={handleSelectChange}
                      options={contextQuery.data?.map(context => ({
                        value: context.name,
                        label: context.name,
                      }))}
                      isLoading={contextQuery.isLoading}
                      styles={selectStyles}
                      onFocus={() =>
                        setUiState(prev => ({
                          ...prev,
                          isContextDropdownFocused: true,
                        }))
                      }
                      onBlur={() =>
                        setUiState(prev => ({
                          ...prev,
                          isContextDropdownFocused: false,
                        }))
                      }
                    />
                    {contextQuery.isError && (
                      <Text color='red.300' fontSize='xs'>
                        Error: select a valid kubeconfig
                      </Text>
                    )}
                  </Stack>
                </Grid>

                <Grid templateColumns='repeat(2, 1fr)' gap={3}>
                  <Stack gap={1.5}>
                    <Text fontSize='xs' color='gray.400'>
                      Workload Type
                    </Text>
                    <Select
                      name='workload_type'
                      value={formState.selectedWorkloadType}
                      onChange={handleSelectChange}
                      options={workloadTypeOptions}
                      styles={selectStyles}
                    />
                  </Stack>

                  <Stack gap={1.5}>
                    <Text fontSize='xs' color='gray.400'>
                      Namespace *
                    </Text>
                    <Select
                      name='namespace'
                      value={formState.selectedNamespace}
                      onChange={handleSelectChange}
                      options={namespaceQuery.data?.map(namespace => ({
                        value: namespace.name,
                        label: namespace.name,
                      }))}
                      isLoading={namespaceQuery.isLoading}
                      styles={selectStyles}
                    />
                    {namespaceQuery.isError && (
                      <Text color='red.300' fontSize='xs'>
                        Error fetching namespaces
                      </Text>
                    )}
                  </Stack>
                </Grid>

                {newConfig.workload_type === 'proxy' ? (
                  <>
                    <Grid templateColumns='repeat(2, 1fr)' gap={3}>
                      <Stack gap={1.5}>
                        <Text fontSize='xs' color='gray.400'>
                          Remote Address
                        </Text>
                        <Input
                          value={newConfig.remote_address || ''}
                          name='remote_address'
                          onChange={handleInputChange}
                          bg='#161616'
                          border='1px solid rgba(255, 255, 255, 0.08)'
                          _hover={{ borderColor: 'rgba(255, 255, 255, 0.15)' }}
                          _focus={{
                            borderColor: 'blue.400',
                            boxShadow: 'none',
                          }}
                          height='28px'
                          fontSize='13px'
                        />
                      </Stack>

                      <Stack gap={1.5}>
                        <Text fontSize='xs' color='gray.400'>
                          Protocol *
                        </Text>
                        <Select
                          name='protocol'
                          value={formState.selectedProtocol}
                          onChange={handleSelectChange}
                          options={protocolOptions}
                          styles={selectStyles}
                        />
                      </Stack>
                    </Grid>

                    <Grid templateColumns='repeat(2, 1fr)' gap={3}>
                      <Stack gap={1.5}>
                        <Text fontSize='xs' color='gray.400'>
                          Target Port *
                        </Text>
                        <Input
                          type='number'
                          value={newConfig.remote_port ?? ''}
                          name='remote_port'
                          onChange={handleInputChange}
                          bg='#161616'
                          border='1px solid rgba(255, 255, 255, 0.08)'
                          _hover={{ borderColor: 'rgba(255, 255, 255, 0.15)' }}
                          _focus={{
                            borderColor: 'blue.400',
                            boxShadow: 'none',
                          }}
                          height='28px'
                          fontSize='13px'
                        />
                      </Stack>

                      <Stack gap={1.5}>
                        <Text fontSize='xs' color='gray.400'>
                          Local Port
                        </Text>
                        <Input
                          type='number'
                          value={newConfig.local_port || ''}
                          name='local_port'
                          onChange={handleInputChange}
                          bg='#161616'
                          border='1px solid rgba(255, 255, 255, 0.08)'
                          _hover={{ borderColor: 'rgba(255, 255, 255, 0.15)' }}
                          _focus={{
                            borderColor: 'blue.400',
                            boxShadow: 'none',
                          }}
                          height='28px'
                          fontSize='13px'
                        />
                      </Stack>
                    </Grid>
                  </>
                ) : (
                  <>
                    <Grid templateColumns='repeat(2, 1fr)' gap={3}>
                      <Stack gap={1.5}>
                        <Text fontSize='xs' color='gray.400'>
                          {newConfig.workload_type === 'pod'
                            ? 'Pod Label'
                            : 'Service'}
                        </Text>
                        <Select
                          name={
                            newConfig.workload_type === 'pod'
                              ? 'target'
                              : 'service'
                          }
                          value={formState.selectedServiceOrTarget}
                          onChange={handleSelectChange}
                          options={
                            newConfig.workload_type === 'pod'
                              ? podsQuery.data?.map(pod => ({
                                value: pod.labels_str,
                                label: pod.labels_str,
                              }))
                              : serviceQuery.data?.map(service => ({
                                value: service.name,
                                label: service.name,
                              }))
                          }
                          isLoading={
                            newConfig.workload_type === 'pod'
                              ? podsQuery.isLoading
                              : serviceQuery.isLoading
                          }
                          styles={selectStyles}
                        />
                        {newConfig.workload_type === 'pod' &&
                          podsQuery.isError && (
                          <Text color='red.300' fontSize='xs'>
                              Error fetching pods
                          </Text>
                        )}
                        {newConfig.workload_type !== 'pod' &&
                          serviceQuery.isError && (
                          <Text color='red.300' fontSize='xs'>
                              Error fetching services
                          </Text>
                        )}
                      </Stack>

                      <Stack gap={1.5}>
                        <Text fontSize='xs' color='gray.400'>
                          Protocol *
                        </Text>
                        <Select
                          name='protocol'
                          value={formState.selectedProtocol}
                          onChange={handleSelectChange}
                          options={protocolOptions}
                          styles={selectStyles}
                        />
                      </Stack>
                    </Grid>

                    <Grid templateColumns='repeat(2, 1fr)' gap={3}>
                      <Stack gap={1.5}>
                        <Text fontSize='xs' color='gray.400'>
                          Target Port *
                        </Text>
                        {formState.selectedPort?.value === CUSTOM_PORT.value ||
                        (newConfig.remote_port &&
                          !portOptions.find(
                            p => p.value === newConfig.remote_port,
                          )) ? (
                            <Input
                              type='number'
                              name='remote_port'
                              value={newConfig.remote_port || ''}
                              onChange={e => {
                                const value = parseInt(e.target.value, 10)

                                if (!isNaN(value)) {
                                  setFormState(prev => ({
                                    ...prev,
                                    selectedPort: CUSTOM_PORT,
                                  }))
                                  const syntheticEvent = {
                                    ...e,
                                    target: {
                                      ...e.target,
                                      name: 'remote_port',
                                      value: value.toString(),
                                    },
                                  }

                                  handleInputChange(syntheticEvent)
                                }
                              }}
                              bg='#161616'
                              border='1px solid rgba(255, 255, 255, 0.08)'
                              _hover={{
                                borderColor: 'rgba(255, 255, 255, 0.15)',
                              }}
                              _focus={{
                                borderColor: 'blue.400',
                                boxShadow: 'none',
                              }}
                              height='28px'
                              fontSize='13px'
                              placeholder='Enter port number'
                            />
                          ) : (
                            <Select
                              name='remote_port'
                              value={formState.selectedPort}
                              onChange={handleSelectChange}
                              options={[CUSTOM_PORT, ...portOptions]}
                              placeholder='Select port'
                              isDisabled={
                                !newConfig.context || !newConfig.namespace
                              }
                              styles={selectStyles}
                            />
                          )}
                        {portQuery.isError && (
                          <Text color='red.300' fontSize='xs'>
                            Error fetching ports
                          </Text>
                        )}
                      </Stack>
                      <Stack gap={1.5}>
                        <Text fontSize='xs' color='gray.400'>
                          Local Port
                        </Text>
                        <Input
                          type='number'
                          value={newConfig.local_port || ''}
                          name='local_port'
                          onChange={handleInputChange}
                          bg='#161616'
                          border='1px solid rgba(255, 255, 255, 0.08)'
                          _hover={{ borderColor: 'rgba(255, 255, 255, 0.15)' }}
                          _focus={{
                            borderColor: 'blue.400',
                            boxShadow: 'none',
                          }}
                          height='28px'
                          fontSize='13px'
                        />
                      </Stack>
                    </Grid>
                  </>
                )}

                <Grid templateColumns='repeat(2, 1fr)' gap={3}>
                  <Stack gap={1.5}>
                    <Checkbox
                      size='xs'
                      checked={uiState.isChecked}
                      onCheckedChange={e =>
                        handleCheckboxChange('local_address_enabled', e)
                      }
                    >
                      <Text fontSize='xs' color='gray.400'>
                        Local Address
                      </Text>
                    </Checkbox>
                    <Input
                      value={newConfig.local_address || '127.0.0.1'}
                      name='local_address'
                      onChange={handleInputChange}
                      disabled={!uiState.isChecked}
                      bg='#161616'
                      border='1px solid rgba(255, 255, 255, 0.08)'
                      _hover={{ borderColor: 'rgba(255, 255, 255, 0.15)' }}
                      _focus={{ borderColor: 'blue.400', boxShadow: 'none' }}
                      height='28px'
                      fontSize='13px'
                      opacity={uiState.isChecked ? 1 : 0.5}
                    />
                  </Stack>
                </Grid>

                <Separator mt={5} />

                <HStack justify='flex-end' gap={2}>
                  <Button
                    size='xs'
                    variant='ghost'
                    onClick={handleCancel}
                    _hover={{ bg: 'whiteAlpha.50' }}
                    height='28px'
                  >
                    Cancel
                  </Button>
                  <Button
                    size='xs'
                    bg='blue.500'
                    _hover={{ bg: 'blue.600' }}
                    type='submit'
                    disabled={!uiState.isFormValid}
                    height='28px'
                  >
                    {isEdit ? 'Save Changes' : 'Add Config'}
                  </Button>
                </HStack>
              </Stack>
            </form>
          </Dialog.Body>
        </Dialog.Content>
      </Dialog.Positioner>
    </Dialog.Root>
  )
}

export default AddConfigModal
