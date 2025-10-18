/* eslint-disable complexity */

import React, { useEffect, useMemo, useState } from 'react'
import { Info } from 'lucide-react'
import Select, { ActionMeta, MultiValue, SingleValue } from 'react-select'

import {
  Button,
  Dialog,
  Flex,
  Grid,
  HStack,
  Input,
  Stack,
  Text,
} from '@chakra-ui/react'
import { useQuery } from '@tanstack/react-query'
import { invoke } from '@tauri-apps/api/core'
import { open } from '@tauri-apps/plugin-dialog'

import { Checkbox } from '@/components/ui/checkbox'
import { toaster } from '@/components/ui/toaster'
import { Tooltip } from '@/components/ui/tooltip'
import {
  Config,
  CustomConfigProps,
  PortOption,
  ServiceData,
  StringOption,
} from '@/types'

import { selectStyles } from './styles'
import { trimConfigValues, validateFormFields } from './utils'

// eslint-disable-next-line max-lines-per-function
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
    { value: 'expose', label: 'Expose' },
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
    if (formState.selectedWorkloadType?.value === 'expose') {
      const exposeRequiredFields = [
        formState.selectedContext?.value ?? null,
        formState.selectedNamespace?.value ?? null,
        newConfig.exposure_type ?? null,
        newConfig.local_port ?? null,
        newConfig.alias ?? null,
      ]

      if (
        newConfig.exposure_type === 'public' &&
        newConfig.cert_manager_enabled
      ) {
        exposeRequiredFields.push(
          newConfig.cert_issuer_kind || 'ClusterIssuer',
          newConfig.cert_issuer ?? null,
        )
      }

      setUiState(prev => ({
        ...prev,
        isFormValid: validateFormFields(exposeRequiredFields),
      }))
    } else {
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
    }
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
    } else if (name === 'auto_loopback_address') {
      setNewConfig(prev => ({
        ...prev,
        auto_loopback_address: isCheckedBoolean,
        local_address: isCheckedBoolean ? '' : '127.0.0.1',
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
    if (isModalOpen && (isEdit || newConfig.context)) {
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
        if (option?.value === 'expose') {
          setNewConfig(prev => ({
            ...prev,
            exposure_type: prev.exposure_type || 'cluster',
            protocol: 'tcp',
          }))
        }
        break
      case 'protocol':
        setFormState(prev => ({
          ...prev,
          selectedProtocol: option,
        }))
        break
      case 'exposure_type':
      case 'cert_issuer_kind':
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
          height='96vh'
          bg='#111111'
          borderRadius='lg'
          border='1px solid rgba(255, 255, 255, 0.08)'
          overflow='hidden'
          position='absolute'
          my={2}
        >
          {/* Compact Header */}
          <Dialog.Header
            p={3}
            bg='#161616'
            borderBottom='1px solid rgba(255, 255, 255, 0.05)'
          >
            <Flex grow='1' justify='space-between' align='center'>
              <Text fontSize='sm' fontWeight='medium' color='gray.100'>
                {isEdit ? 'Edit Configuration' : 'Add Configuration'}
              </Text>

              {/* Kubeconfig Section */}
              <HStack gap={2}>
                <Text fontSize='2xs' color='gray.400'>
                  Kubeconfig:
                </Text>
                <Tooltip content={uiState.kubeConfig} portalled>
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
                </Tooltip>
              </HStack>
            </Flex>
          </Dialog.Header>

          <form onSubmit={handleSave} style={{ display: 'contents' }}>
            <Dialog.Body
              p={3}
              overflowY='auto'
              css={{
                '&::-webkit-scrollbar': {
                  width: '6px',
                },
                '&::-webkit-scrollbar-track': {
                  background: 'transparent',
                },
                '&::-webkit-scrollbar-thumb': {
                  background: 'rgba(255, 255, 255, 0.2)',
                  borderRadius: '3px',
                },
                '&::-webkit-scrollbar-thumb:hover': {
                  background: 'rgba(255, 255, 255, 0.3)',
                },
              }}
            >
              <Stack gap={2}>
                <Grid templateColumns='repeat(2, 1fr)' gap={3}>
                  <Stack gap={1.5}>
                    <Flex align='center' gap={1}>
                      <Text fontSize='xs' color='gray.400'>
                        {newConfig.workload_type === 'expose'
                          ? 'Domain *'
                          : 'Alias'}
                      </Text>
                      {newConfig.workload_type === 'expose' && (
                        <Tooltip
                          content={
                            newConfig.exposure_type === 'public'
                              ? 'Full domain for public access (e.g., myapp.example.com). The Kubernetes service will be named using the first part before the dot (e.g., "myapp").'
                              : `Service name in cluster (accessible as ${newConfig.alias || 'name'}.${newConfig.namespace || 'namespace'}.svc.cluster.local)`
                          }
                          portalled
                        >
                          <span
                            style={{
                              display: 'inline-flex',
                              alignItems: 'center',
                            }}
                          >
                            <Info size={10} color='#6B7280' />
                          </span>
                        </Tooltip>
                      )}
                    </Flex>
                    <Input
                      value={newConfig.alias || ''}
                      name='alias'
                      onChange={handleInputChange}
                      placeholder={
                        newConfig.workload_type === 'expose'
                          ? newConfig.exposure_type === 'public'
                            ? 'myapp.example.com'
                            : 'my-service'
                          : ''
                      }
                      bg='#161616'
                      border='1px solid rgba(255, 255, 255, 0.08)'
                      _hover={{ borderColor: 'rgba(255, 255, 255, 0.15)' }}
                      _focus={{ borderColor: 'blue.400', boxShadow: 'none' }}
                      height='28px'
                      fontSize='13px'
                    />
                    {newConfig.workload_type !== 'expose' && (
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
                    )}
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

                {/* Expose-specific options */}
                {newConfig.workload_type === 'expose' && (
                  <Stack gap={3}>
                    {/* Exposure Type */}
                    <Stack gap={1.5}>
                      <Flex align='center' gap={1}>
                        <Text fontSize='xs' color='gray.400'>
                          Exposure Type *
                        </Text>
                        <Tooltip
                          content='Cluster Only: Accessible within cluster via DNS. Public: Exposed to internet via Ingress.'
                          portalled
                        >
                          <span
                            style={{
                              display: 'inline-flex',
                              alignItems: 'center',
                            }}
                          >
                            <Info size={10} color='#6B7280' />
                          </span>
                        </Tooltip>
                      </Flex>
                      <Select
                        name='exposure_type'
                        value={
                          newConfig.exposure_type
                            ? {
                                value: newConfig.exposure_type,
                                label:
                                  newConfig.exposure_type === 'cluster'
                                    ? 'Cluster Only (Internal)'
                                    : 'Public (Internet)',
                              }
                            : {
                                value: 'cluster',
                                label: 'Cluster Only (Internal)',
                              }
                        }
                        onChange={handleSelectChange}
                        options={[
                          {
                            value: 'cluster',
                            label: 'Cluster Only (Internal)',
                          },
                          { value: 'public', label: 'Public (Internet)' },
                        ]}
                        styles={selectStyles}
                      />
                    </Stack>

                    {/* Local Port */}
                    <Stack gap={1.5}>
                      <Flex align='center' gap={1}>
                        <Text fontSize='xs' color='gray.400'>
                          Local Port *
                        </Text>
                        <Tooltip
                          content='Port of your local development server'
                          portalled
                        >
                          <span
                            style={{
                              display: 'inline-flex',
                              alignItems: 'center',
                            }}
                          >
                            <Info size={10} color='#6B7280' />
                          </span>
                        </Tooltip>
                      </Flex>
                      <Input
                        type='number'
                        name='local_port'
                        value={newConfig.local_port || ''}
                        onChange={handleInputChange}
                        placeholder='3000'
                        bg='#161616'
                        border='1px solid rgba(255, 255, 255, 0.08)'
                        _hover={{ borderColor: 'rgba(255, 255, 255, 0.15)' }}
                        _focus={{ borderColor: 'blue.400', boxShadow: 'none' }}
                        height='28px'
                        fontSize='13px'
                      />
                    </Stack>

                    {/* Local Address */}
                    <Stack gap={1.5}>
                      <Flex align='center' gap={1}>
                        <Text fontSize='xs' color='gray.400'>
                          Local Address
                        </Text>
                        <Tooltip
                          content='Address where your local service is running (default: 127.0.0.1)'
                          portalled
                        >
                          <span
                            style={{
                              display: 'inline-flex',
                              alignItems: 'center',
                            }}
                          >
                            <Info size={10} color='#6B7280' />
                          </span>
                        </Tooltip>
                      </Flex>
                      <Input
                        name='local_address'
                        value={newConfig.local_address || '127.0.0.1'}
                        onChange={handleInputChange}
                        placeholder='127.0.0.1'
                        bg='#161616'
                        border='1px solid rgba(255, 255, 255, 0.08)'
                        _hover={{ borderColor: 'rgba(255, 255, 255, 0.15)' }}
                        _focus={{ borderColor: 'blue.400', boxShadow: 'none' }}
                        height='28px'
                        fontSize='13px'
                      />
                    </Stack>

                    {/* cert-manager - Only for public */}
                    {newConfig.exposure_type === 'public' && (
                      <>
                        <Flex align='center' gap={1}>
                          <Checkbox
                            size='xs'
                            checked={newConfig.cert_manager_enabled || false}
                            onCheckedChange={e => {
                              const isChecked =
                                e.checked === 'indeterminate'
                                  ? false
                                  : e.checked

                              setNewConfig({
                                ...newConfig,
                                cert_manager_enabled: isChecked,
                                domain_enabled: isChecked,
                                // Set defaults when enabling cert-manager
                                cert_issuer_kind: isChecked
                                  ? newConfig.cert_issuer_kind ||
                                    'ClusterIssuer'
                                  : newConfig.cert_issuer_kind,
                              })
                            }}
                          >
                            <Text fontSize='xs' color='gray.400'>
                              Enable HTTPS (cert-manager)
                            </Text>
                          </Checkbox>
                          <Tooltip
                            content='Automatically provision TLS certificate using cert-manager'
                            portalled
                          >
                            <span
                              style={{
                                display: 'inline-flex',
                                alignItems: 'center',
                              }}
                            >
                              <Info size={10} color='#6B7280' />
                            </span>
                          </Tooltip>
                        </Flex>

                        {newConfig.cert_manager_enabled && (
                          <>
                            <Stack gap={1.5}>
                              <Text fontSize='xs' color='gray.400'>
                                Issuer Kind *
                              </Text>
                              <Select
                                name='cert_issuer_kind'
                                value={
                                  newConfig.cert_issuer_kind
                                    ? {
                                        value: newConfig.cert_issuer_kind,
                                        label: newConfig.cert_issuer_kind,
                                      }
                                    : {
                                        value: 'ClusterIssuer',
                                        label: 'ClusterIssuer',
                                      }
                                }
                                onChange={handleSelectChange}
                                options={[
                                  {
                                    value: 'ClusterIssuer',
                                    label: 'ClusterIssuer',
                                  },
                                  { value: 'Issuer', label: 'Issuer' },
                                ]}
                                styles={selectStyles}
                              />
                            </Stack>

                            <Stack gap={1.5}>
                              <Flex align='center' gap={1}>
                                <Text fontSize='xs' color='gray.400'>
                                  Issuer Name *
                                </Text>
                                <Tooltip
                                  content='Name of your cert-manager issuer (e.g., letsencrypt-prod)'
                                  portalled
                                >
                                  <span
                                    style={{
                                      display: 'inline-flex',
                                      alignItems: 'center',
                                    }}
                                  >
                                    <Info size={10} color='#6B7280' />
                                  </span>
                                </Tooltip>
                              </Flex>
                              <Input
                                name='cert_issuer'
                                value={newConfig.cert_issuer || ''}
                                onChange={handleInputChange}
                                placeholder='letsencrypt-prod'
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
                              />
                            </Stack>
                          </>
                        )}

                        <Stack gap={1.5}>
                          <Flex align='center' gap={1}>
                            <Text fontSize='xs' color='gray.400'>
                              Ingress Class (Optional)
                            </Text>
                            <Tooltip
                              content='Leave empty to use default ingress class (e.g., nginx, traefik)'
                              portalled
                            >
                              <span
                                style={{
                                  display: 'inline-flex',
                                  alignItems: 'center',
                                }}
                              >
                                <Info size={10} color='#6B7280' />
                              </span>
                            </Tooltip>
                          </Flex>
                          <Input
                            name='ingress_class'
                            value={newConfig.ingress_class || ''}
                            onChange={handleInputChange}
                            placeholder='nginx'
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
                          />
                        </Stack>

                        <Stack gap={1.5}>
                          <Flex align='center' gap={1}>
                            <Text fontSize='xs' color='gray.400'>
                              Additional Annotations (Optional)
                            </Text>
                            <Tooltip
                              content='JSON format: {"key": "value"}. Example: nginx.ingress.kubernetes.io/rewrite-target'
                              portalled
                            >
                              <span
                                style={{
                                  display: 'inline-flex',
                                  alignItems: 'center',
                                }}
                              >
                                <Info size={10} color='#6B7280' />
                              </span>
                            </Tooltip>
                          </Flex>
                          <Input
                            name='ingress_annotations'
                            value={newConfig.ingress_annotations || ''}
                            onChange={handleInputChange}
                            placeholder='{"key1": "value1", "key2": "value2"}'
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
                          />
                        </Stack>
                      </>
                    )}
                  </Stack>
                )}

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

                    <Grid templateColumns='repeat(2, 1fr)' gap={3}>
                      <Stack gap={1.5}>
                        <Text fontSize='xs' color='gray.400'>
                          Local Address (Optional)
                        </Text>
                        <Input
                          value={
                            newConfig.auto_loopback_address
                              ? ''
                              : newConfig.local_address || '127.0.0.1'
                          }
                          name='local_address'
                          onChange={handleInputChange}
                          disabled={newConfig.auto_loopback_address}
                          bg='#161616'
                          border='1px solid rgba(255, 255, 255, 0.08)'
                          _hover={{ borderColor: 'rgba(255, 255, 255, 0.15)' }}
                          _focus={{
                            borderColor: 'blue.400',
                            boxShadow: 'none',
                          }}
                          height='28px'
                          fontSize='13px'
                          opacity={newConfig.auto_loopback_address ? 0.5 : 1}
                          placeholder={
                            newConfig.auto_loopback_address
                              ? '127.0.0.x'
                              : 'e.g., 127.0.0.1'
                          }
                        />
                        <Checkbox
                          size='xs'
                          checked={newConfig.auto_loopback_address || false}
                          onCheckedChange={e =>
                            handleCheckboxChange('auto_loopback_address', e)
                          }
                        >
                          <Text fontSize='xs' color='gray.400'>
                            Auto select address
                          </Text>
                        </Checkbox>
                      </Stack>
                    </Grid>
                  </>
                ) : newConfig.workload_type !== 'expose' ? (
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
                      <Stack gap={1.5}>
                        <Text fontSize='xs' color='gray.400'>
                          Local Address (Optional)
                        </Text>
                        <Input
                          value={
                            newConfig.auto_loopback_address
                              ? ''
                              : newConfig.local_address || '127.0.0.1'
                          }
                          name='local_address'
                          onChange={handleInputChange}
                          disabled={newConfig.auto_loopback_address}
                          bg='#161616'
                          border='1px solid rgba(255, 255, 255, 0.08)'
                          _hover={{ borderColor: 'rgba(255, 255, 255, 0.15)' }}
                          _focus={{
                            borderColor: 'blue.400',
                            boxShadow: 'none',
                          }}
                          height='28px'
                          fontSize='13px'
                          opacity={newConfig.auto_loopback_address ? 0.5 : 1}
                          placeholder={
                            newConfig.auto_loopback_address
                              ? '127.0.0.x'
                              : 'e.g., 127.0.0.1'
                          }
                        />
                        <Checkbox
                          size='xs'
                          checked={newConfig.auto_loopback_address || false}
                          onCheckedChange={e =>
                            handleCheckboxChange('auto_loopback_address', e)
                          }
                        >
                          <Text fontSize='xs' color='gray.400'>
                            Auto select address
                          </Text>
                        </Checkbox>
                      </Stack>
                    </Grid>
                  </>
                ) : null}
              </Stack>
            </Dialog.Body>

            <Dialog.Footer
              p={2}
              bg='#161616'
              borderTop='1px solid rgba(255, 255, 255, 0.05)'
            >
              <Flex justify='flex-end' gap={2} width='100%'>
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
              </Flex>
            </Dialog.Footer>
          </form>
        </Dialog.Content>
      </Dialog.Positioner>
    </Dialog.Root>
  )
}

export default AddConfigModal
