/* eslint-disable max-lines-per-function */

/* eslint-disable complexity */

import React, { useEffect, useMemo, useState } from 'react'
import { InfoIcon } from 'lucide-react'
import { useQuery } from 'react-query'
import Select, { ActionMeta } from 'react-select'

import {
  Button,
  Checkbox as C,
  Dialog,
  Field as F,
  Flex,
  Grid,
  HStack,
  Input,
  Separator,
  Stack,
  Text,
  Tooltip,
} from '@chakra-ui/react'
import { open } from '@tauri-apps/api/dialog'
import { invoke } from '@tauri-apps/api/tauri'

import { fetchKubeContexts } from '@/components/PortForwardTable/ContextsAccordion/utils'
import { useCustomToast } from '@/components/ui/toaster'
import { Config, CustomConfigProps, KubeContext } from '@/types'

// Define our option types
interface StringOption {
  value: string
  label: string
}

interface PortOption {
  value: number
  label: string
}

// Define the service interface
interface ServiceData {
  name: string
  port?: number
}

const AddConfigModal: React.FC<CustomConfigProps> = ({
  isModalOpen,
  closeModal,
  newConfig,
  handleInputChange,
  handleSaveConfig,
  isEdit,
  configData,
  setNewConfig,
}) => {
  const [selectedContext, setSelectedContext] = useState<StringOption | null>(
    null,
  )
  const [selectedNamespace, setSelectedNamespace] =
    useState<StringOption | null>(null)
  const [selectedServiceOrTarget, setSelectedServiceOrTarget] =
    useState<StringOption | null>(null)
  const [selectedPort, setSelectedPort] = useState<PortOption | null>(null)
  const [selectedWorkloadType, setSelectedWorkloadType] =
    useState<StringOption | null>(null)
  const [selectedProtocol, setSelectedProtocol] = useState<StringOption | null>(
    null,
  )
  const showToast = useCustomToast()
  const [isChecked, setIsChecked] = useState(false)
  const [isContextDropdownFocused, setIsContextDropdownFocused] =
    useState(false)

  const handleCheckboxChange = (e: React.FormEvent<HTMLLabelElement>) => {
    const target = e.target as HTMLInputElement

    setNewConfig(prev => ({
      ...prev,
      [target.name]: target.checked,
    }))
  }

  useEffect(() => {
    const isValid = [
      selectedContext,
      selectedNamespace,
      selectedServiceOrTarget ?? newConfig.remote_address,
      selectedPort ?? newConfig.remote_port,
      selectedWorkloadType,
      selectedProtocol,
    ].every(field => field !== null && field !== '')

    setIsFormValid(isValid)
  }, [
    selectedContext,
    selectedNamespace,
    selectedServiceOrTarget,
    selectedPort,
    selectedWorkloadType,
    selectedProtocol,
    newConfig.remote_address,
    newConfig.remote_port,
    newConfig.workload_type,
    newConfig.target,
  ])

  const [isFormValid, setIsFormValid] = useState(false)
  const [kubeConfig, setKubeConfig] = useState<string>('default')

  const contextQuery = useQuery<KubeContext[]>(
    ['kube-contexts', kubeConfig],
    () => fetchKubeContexts(kubeConfig),
    {
      enabled:
        isModalOpen && (isContextDropdownFocused || kubeConfig !== 'default'),
      onError: error => {
        console.error('Error fetching contexts:', error)
        if (error instanceof Error) {
          showToast({
            title: 'Error fetching contexts',
            description: error.message,
            status: 'error',
          })
        } else {
          showToast({
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
    }
  }

  const namespaceQuery = useQuery(
    ['kube-namespaces', newConfig.context],
    () => {
      return invoke<{ name: string }[]>('list_namespaces', {
        contextName: newConfig.context,
        kubeconfig: kubeConfig,
      })
    },
    {
      initialData: configData?.namespace,
      enabled: isModalOpen && !!newConfig.context,
      onError: error => {
        console.error('Error fetching namespaces:', error)
        if (error instanceof Error) {
          showToast({
            title: 'Error fetching namespaces',
            description: error.message,
            status: 'error',
          })
        } else {
          showToast({
            title: 'Error fetching namespaces',
            description: 'An unknown error occurred',
            status: 'error',
          })
        }
      },
    },
  )
  const podsQuery = useQuery(
    ['kube-pods', newConfig.context, newConfig.namespace],
    () => {
      return invoke<{ labels_str: string }[]>('list_pods', {
        contextName: newConfig.context,
        namespace: newConfig.namespace,
        kubeconfig: kubeConfig,
      })
    },
    {
      enabled:
        isModalOpen &&
        !!newConfig.context &&
        !!newConfig.namespace &&
        newConfig.workload_type === 'pod',
      onError: error => {
        console.error('Error fetching pods:', error)
        if (error instanceof Error) {
          showToast({
            title: 'Error fetching pods',
            description: error.message,
            status: 'error',
          })
        } else {
          showToast({
            title: 'Error fetching pods',
            description: 'An unknown error occurred',
            status: 'error',
          })
        }
      },
    },
  )

  const serviceOrTargetQuery = useQuery<ServiceData[]>(
    ['services', newConfig.context, newConfig.namespace],
    () => {
      return invoke<ServiceData[]>('list_services', {
        contextName: newConfig.context,
        namespace: newConfig.namespace,
        kubeconfig: kubeConfig,
      })
    },
    {
      enabled:
        isModalOpen &&
        !!newConfig.context &&
        !!newConfig.namespace &&
        newConfig.workload_type === 'service',
    },
  )

  const portQuery = useQuery(
    [
      'kube-service-ports',
      newConfig.context,
      newConfig.namespace,
      newConfig.workload_type === 'pod' ? newConfig.target : newConfig.service,
    ],
    () => {
      if (newConfig.workload_type === 'pod') {
        return invoke<{ name: string; port: number }[]>('list_ports', {
          contextName: newConfig.context,
          namespace: newConfig.namespace,
          serviceName: newConfig.target,
          kubeconfig: kubeConfig,
        })
      }

      return invoke<{ name: string; port: number }[]>('list_ports', {
        contextName: newConfig.context,
        namespace: newConfig.namespace,
        serviceName: newConfig.service ?? newConfig.target,
        kubeconfig: kubeConfig,
      })
    },
    {
      initialData:
        configData?.ports?.map(port => ({
          name: port.name ?? 'default',
          port: port.port,
        })) ?? [],
      enabled:
        isModalOpen &&
        !!newConfig.context &&
        !!newConfig.namespace &&
        !!(newConfig.workload_type === 'pod'
          ? newConfig.target
          : newConfig.service) &&
        newConfig.workload_type !== 'proxy',
      onSuccess: ports => {
        console.log('Ports fetched successfully:', ports)
      },
      onError: error => {
        console.error('Error fetching service ports:', error)
        if (error instanceof Error) {
          showToast({
            title: 'Error fetching service ports',
            description: error.message,
            status: 'error',
          })
        } else {
          showToast({
            title: 'Error fetching service ports',
            description: 'An unknown error occurred',
            status: 'error',
          })
        }
      },
    },
  )


  useEffect(() => {
    if (isEdit && isModalOpen) {
      setSelectedWorkloadType(
        newConfig.workload_type
          ? { label: newConfig.workload_type, value: newConfig.workload_type }
          : null,
      )
      setSelectedProtocol(
        newConfig.protocol
          ? { label: newConfig.protocol, value: newConfig.protocol }
          : null,
      )
      setSelectedContext(
        newConfig.context
          ? { label: newConfig.context, value: newConfig.context }
          : null,
      )
      setSelectedNamespace(
        newConfig.namespace
          ? { label: newConfig.namespace, value: newConfig.namespace }
          : null,
      )
      setSelectedServiceOrTarget(
        newConfig.service
          ? { label: newConfig.service, value: newConfig.service }
          : newConfig.target
            ? { label: newConfig.target, value: newConfig.target }
            : null,
      )
      setSelectedPort(
        newConfig.remote_port
          ? {
            label: newConfig.remote_port.toString(),
            value: newConfig.remote_port,
          }
          : null,
      )
      setKubeConfig(newConfig.kubeconfig ?? 'default')
    }
  }, [
    isEdit,
    isModalOpen,
    newConfig.context,
    newConfig.namespace,
    newConfig.service,
    newConfig.protocol,
    newConfig.remote_port,
    newConfig.workload_type,
    newConfig.kubeconfig,
    newConfig.target,
  ])

  useEffect(() => {
    if (!isModalOpen) {
      resetState()
    }
  }, [isModalOpen])

  const resetState = () => {
    setSelectedContext(null)
    setSelectedNamespace(null)
    setSelectedServiceOrTarget(null)
    setSelectedPort(null)
    setSelectedWorkloadType(null)
    setSelectedProtocol(null)
    setKubeConfig('default')
    setIsContextDropdownFocused(false)
  }

  const handleSelectChange = (
    selectedOption: StringOption | PortOption | null,
    { name }: ActionMeta<StringOption | PortOption>,
  ) => {
    // Handle port separately first
    if (name === 'remote_port') {
      const portOption = selectedOption as PortOption | null

      setSelectedPort(portOption)
      setNewConfig(prev => ({
        ...prev,
        remote_port: portOption?.value,
      }))

      return
    }

    // Handle string-based options
    switch (name) {
    case 'context':
      setSelectedContext(selectedOption as StringOption | null)
      break
    case 'namespace':
      setSelectedNamespace(selectedOption as StringOption | null)
      break
    case 'service':
    case 'target':
      setSelectedServiceOrTarget(selectedOption as StringOption | null)
      break
    case 'workload_type':
      setSelectedWorkloadType(selectedOption as StringOption | null)
      break
    case 'protocol':
      setSelectedProtocol(selectedOption as StringOption | null)
      break
    }

    // Update the config
    handleInputChange({
      target: {
        name: name,
        value: selectedOption ? selectedOption.value : '',
      } as HTMLInputElement,
    } as React.ChangeEvent<HTMLInputElement>)
  }

  useEffect(() => {
    if (setNewConfig) {
      setNewConfig((prevConfig: Config) => ({
        ...prevConfig,
        kubeconfig: kubeConfig ?? '',
      }))
    }
  }, [kubeConfig, setNewConfig])
  const trimConfigValues = (config: Config): Config => {
    const trimmedConfig: Config = { ...config }

    // Type assertion to allow string indexing
    Object.keys(config).forEach(key => {
      if (typeof (config as any)[key] === 'string') {
        ;(trimmedConfig as any)[key] = (config as any)[key].trim()
      }
    })

    return trimmedConfig
  }

  const handleSave = async (event: React.FormEvent) => {
    event.preventDefault()

    const configToSave: Config = {
      ...newConfig,
      kubeconfig: kubeConfig ?? '',
    }

    const trimmedConfig = trimConfigValues(configToSave)

    await handleSaveConfig(trimmedConfig)

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
    () =>
      serviceOrTargetQuery.data
      ?.filter(
        (service: ServiceData): service is ServiceData & { port: number } =>
          service.port !== undefined && typeof service.port === 'number',
      )
      .map(service => ({
        value: service.port,
        label: service.port.toString(),
      })) || [],
    [serviceOrTargetQuery.data],
  )



  return (
    <Dialog.Root open={isModalOpen} onOpenChange={handleCancel}>
	  <Dialog.Backdrop bg="blackAlpha.500" backdropFilter="blur(8px)" borderRadius="lg" />
	  <Dialog.Positioner>
        <Dialog.Content
		  onClick={(e) => e.stopPropagation()}
		  maxWidth="400px"
		  width="200vw"
		  maxHeight="200vh"
		  bg="#111111"
		  borderRadius="lg"
		  border="1px solid rgba(255, 255, 255, 0.08)"
		  overflow="hidden"
		  mt={5}
        >
		  <Dialog.Header
            p={4}
            bg="#161616"
            borderBottom="1px solid rgba(255, 255, 255, 0.05)"
		  >
            <Text fontSize="sm" fontWeight="medium" color="gray.100">
			  {isEdit ? 'Edit Configuration' : 'Add Configuration'}
            </Text>
		  </Dialog.Header>

		  <Dialog.Body p={4}>
            <form onSubmit={handleSave}>
			  <Stack gap={5}>
                {/* Kubeconfig Section */}
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

                {/* Alias and Domain Section */}
                <Grid templateColumns="repeat(2, 1fr)" gap={4}>
				  <Stack gap={2}>
                    <Text fontSize="xs" color="gray.400">Alias</Text>
                    <Input
					  value={newConfig.alias || ''}
					  name="alias"
					  onChange={handleInputChange}
					  bg="#161616"
					  border="1px solid rgba(255, 255, 255, 0.08)"
					  _hover={{ borderColor: 'rgba(255, 255, 255, 0.15)' }}
					  _focus={{ borderColor: 'blue.400', boxShadow: 'none' }}
					  height="32px"
					  fontSize="13px"
                    />
                    <C.Root
					  checked={newConfig.domain_enabled || false}
					  onChange={handleCheckboxChange}
					  name="domain_enabled"
                    >
					  <C.Control bg="#161616" borderColor="whiteAlpha.200" />
					  <C.Label>
                        <Text fontSize="xs" color="gray.400">Enable alias as domain</Text>
					  </C.Label>
                    </C.Root>
				  </Stack>

				  <Stack gap={2}>
                    <Text fontSize="xs" color="gray.400">Context *</Text>
                    <Select
					  name="context"
					  value={selectedContext}
					  onChange={(option, action) => handleSelectChange(option, action)}
					  options={contextQuery.data?.map(context => ({
                        value: context.name,
                        label: context.name,
					  }))}
					  isLoading={contextQuery.isLoading}
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
                    {contextQuery.isError && (
					  <Text color="red.300" fontSize="xs">
						Error: select a valid kubeconfig
					  </Text>
                    )}
				  </Stack>
                </Grid>

                {/* Workload Type and Namespace Section */}
                <Grid templateColumns="repeat(2, 1fr)" gap={4}>
				  <Stack gap={2}>
                    <Text fontSize="xs" color="gray.400">Workload Type</Text>
                    <Select
					  name="workload_type"
					  value={selectedWorkloadType}
					  onChange={(option, action) => handleSelectChange(option, action)}
					  options={[
                        { value: 'service', label: 'service' },
                        { value: 'proxy', label: 'proxy' },
                        { value: 'pod', label: 'pod' },
					  ]}
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
				  </Stack>

				  <Stack gap={2}>
                    <Text fontSize="xs" color="gray.400">Namespace *</Text>
                    <Select
					  name="namespace"
					  value={selectedNamespace}
					  onChange={(option, action) => handleSelectChange(option, action)}
					  options={namespaceQuery.data?.map(namespace => ({
                        value: namespace.name,
                        label: namespace.name,
					  }))}
					  isLoading={namespaceQuery.isLoading}
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
                    {namespaceQuery.isError && (
					  <Text color="red.300" fontSize="xs">
						Error fetching namespaces
					  </Text>
                    )}
				  </Stack>
                </Grid>

                {/* Conditional Rendering based on workload_type */}
                {newConfig.workload_type?.startsWith('proxy') ? (
				  <>
                    <Grid templateColumns="repeat(2, 1fr)" gap={4}>
					  <Stack gap={2}>
                        <Text fontSize="xs" color="gray.400">Remote Address</Text>
                        <Input
						  value={newConfig.remote_address || ''}
						  name="remote_address"
						  onChange={handleInputChange}
						  bg="#161616"
						  border="1px solid rgba(255, 255, 255, 0.08)"
						  _hover={{ borderColor: 'rgba(255, 255, 255, 0.15)' }}
						  _focus={{ borderColor: 'blue.400', boxShadow: 'none' }}
						  height="32px"
						  fontSize="13px"
                        />
					  </Stack>

					  <Stack gap={2}>
                        <Text fontSize="xs" color="gray.400">Protocol *</Text>
                        <Select
						  name="protocol"
						  value={selectedProtocol}
						  onChange={(option, action) => handleSelectChange(option, action)}
						  options={[
                            { value: 'udp', label: 'udp' },
                            { value: 'tcp', label: 'tcp' },
						  ]}
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
					  </Stack>
                    </Grid>

                    <Grid templateColumns="repeat(2, 1fr)" gap={4}>
					  <Stack gap={2}>
                        <Text fontSize="xs" color="gray.400">Target Port *</Text>
                        <Input
						  type="number"
						  value={newConfig.remote_port || ''}
						  name="remote_port"
						  onChange={handleInputChange}
						  bg="#161616"
						  border="1px solid rgba(255, 255, 255, 0.08)"
						  _hover={{ borderColor: 'rgba(255, 255, 255, 0.15)' }}
						  _focus={{ borderColor: 'blue.400', boxShadow: 'none' }}
						  height="32px"
						  fontSize="13px"
                        />
					  </Stack>

					  <Stack gap={2}>
                        <Text fontSize="xs" color="gray.400">Local Port</Text>
                        <Input
						  type="number"
						  value={newConfig.local_port || ''}
						  name="local_port"
						  onChange={handleInputChange}
						  bg="#161616"
						  border="1px solid rgba(255, 255, 255, 0.08)"
						  _hover={{ borderColor: 'rgba(255, 255, 255, 0.15)' }}
						  _focus={{ borderColor: 'blue.400', boxShadow: 'none' }}
						  height="32px"
						  fontSize="13px"
                        />
					  </Stack>
                    </Grid>
				  </>
                ) : (
				  <>
                    <Grid templateColumns="repeat(2, 1fr)" gap={4}>
					  <Stack gap={2}>
                        <Text fontSize="xs" color="gray.400">
						  {newConfig.workload_type === 'pod' ? 'Pod Label' : 'Service'}
                        </Text>
                        <Select
						  name={newConfig.workload_type === 'pod' ? 'target' : 'service'}
						  value={selectedServiceOrTarget}
						  onChange={(option, action) => handleSelectChange(option, action)}
						  options={
                            newConfig.workload_type === 'pod'
							  ? podsQuery.data?.map(pod => ({
                                value: pod.labels_str,
                                label: pod.labels_str,
							  }))
							  : serviceOrTargetQuery.data?.map(service => ({
                                value: service.name,
                                label: service.name,
							  }))
						  }
						  isLoading={
                            newConfig.workload_type === 'pod'
							  ? podsQuery.isLoading
							  : serviceOrTargetQuery.isLoading
						  }
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
                        {newConfig.workload_type === 'pod' && podsQuery.isError && (
						  <Text color="red.300" fontSize="xs">
							Error fetching pods
						  </Text>
                        )}
                        {newConfig.workload_type !== 'pod' && serviceOrTargetQuery.isError && (
						  <Text color="red.300" fontSize="xs">
							Error fetching services or targets
						  </Text>
                        )}
					  </Stack>

					  <Stack gap={2}>
                        <Text fontSize="xs" color="gray.400">Protocol *</Text>
                        <Select
						  name="protocol"
						  value={selectedProtocol}
						  onChange={(option, action) => handleSelectChange(option, action)}
						  options={[
                            { value: 'udp', label: 'udp' },
                            { value: 'tcp', label: 'tcp' },
						  ]}
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
					  </Stack>
                    </Grid>

                    <Grid templateColumns="repeat(2, 1fr)" gap={4}>
					  <Stack gap={2}>
                        <Text fontSize="xs" color="gray.400">Target Port *</Text>
                        <Select
						  name="remote_port"
						  value={selectedPort}
						  onChange={(option, action) => handleSelectChange(option, action)}
						  options={portOptions}
						  placeholder="Select port"
						  isDisabled={!newConfig.context || !newConfig.namespace}
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
                        {portQuery.isError && (
						  <Text color="red.300" fontSize="xs">
							Error fetching ports
						  </Text>
                        )}
					  </Stack>

					  <Stack gap={2}>
                        <Text fontSize="xs" color="gray.400">Local Port</Text>
                        <Input
						  type="number"
						  value={newConfig.local_port || ''}
						  name="local_port"
						  onChange={handleInputChange}
						  bg="#161616"
						  border="1px solid rgba(255, 255, 255, 0.08)"
						  _hover={{ borderColor: 'rgba(255, 255, 255, 0.15)' }}
						  _focus={{ borderColor: 'blue.400', boxShadow: 'none' }}
						  height="32px"
						  fontSize="13px"
                        />
					  </Stack>
                    </Grid>
				  </>
                )}

                {/* Local Address Section */}
                <Grid templateColumns="repeat(2, 1fr)" gap={4}>
				  <Stack gap={2}>
                    <C.Root
					  checked={isChecked}
					  onChange={(e) => {
                        const target = e.target as HTMLInputElement


                        setIsChecked(target.checked)
					  }}
                    >
					  <C.Control bg="#161616" borderColor="whiteAlpha.200" />
					  <C.Label>
                        <Text fontSize="xs" color="gray.400">Local Address</Text>
					  </C.Label>
                    </C.Root>
                    <Input
					  value={newConfig.local_address || '127.0.0.1'}
					  name="local_address"
					  onChange={handleInputChange}
					  disabled={!isChecked}
					  bg="#161616"
					  border="1px solid rgba(255, 255, 255, 0.08)"
					  _hover={{ borderColor: 'rgba(255, 255, 255, 0.15)' }}
					  _focus={{ borderColor: 'blue.400', boxShadow: 'none' }}
					  height="32px"
					  fontSize="13px"
					  opacity={isChecked ? 1 : 0.5}
                    />
				  </Stack>
                </Grid>

                {/* Footer Actions */}
                <HStack justify="flex-end" pt={4} gap={3}>
				  <Button
                    size="sm"
                    variant="ghost"
                    onClick={handleCancel}
                    _hover={{ bg: 'whiteAlpha.50' }}
                    height="32px"
				  >
					Cancel
				  </Button>
				  <Button
                    size="sm"
                    bg="blue.500"
                    _hover={{ bg: 'blue.600' }}
                    type="submit"
                    disabled={!isFormValid}
                    height="32px"
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
