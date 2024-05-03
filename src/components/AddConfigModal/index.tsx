import React, { useEffect, useState } from 'react'
import { useQuery } from 'react-query'
import ReactSelect, { ActionMeta } from 'react-select'

import {
  Box,
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
  Text,
  Tooltip,
  useToast,
} from '@chakra-ui/react'
import { open } from '@tauri-apps/api/dialog'
import { invoke } from '@tauri-apps/api/tauri'

import theme from '../../assets/theme'
import { Config, CustomConfigProps, KubeContext, Option } from '../../types'

import { customStyles, fetchKubeContexts } from './utils'

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
  const [selectedContext, setSelectedContext] = useState<{
    name?: string
    value: string | number
    label: string
  } | null>(null)
  const [selectedNamespace, setSelectedNamespace] = useState<{
    name?: string
    value: string | number
    label: string
  } | null>(null)
  const [selectedService, setSelectedService] = useState<{
    name?: string
    value: string | number
    label: string
  } | null>(null)
  const [selectedPort, setSelectedPort] = useState<{
    name?: string
    value: string | number
    label: string
  } | null>(null)
  const [selectedWorkloadType, setSelectedWorkloadType] = useState<{
    name?: string
    value: string | number
    label: string
  } | null>(null)
  const [selectedProtocol, setSelectedProtocol] = useState<{
    name?: string
    value: string | number
    label: string
  } | null>(null)

  const [isChecked, setIsChecked] = useState(false)
  const handleCheckboxChange = () => {
    setIsChecked(!isChecked)
  }

  useEffect(() => {
    const isValid = [
      selectedContext,
      selectedNamespace,
      selectedService ?? newConfig.remote_address,
      selectedPort ?? newConfig.remote_port,
      selectedWorkloadType,
      selectedProtocol,
      newConfig.alias,
      newConfig.local_port,
    ].every(field => field !== null && field !== '')

    setIsFormValid(isValid)
  }, [
    selectedContext,
    selectedNamespace,
    selectedService,
    selectedPort,
    selectedWorkloadType,
    selectedProtocol,
    newConfig.alias,
    newConfig.remote_address,
    newConfig.remote_port,
    newConfig.local_port,
    newConfig.workload_type,
  ])

  const [isFormValid, setIsFormValid] = useState(false)
  const [kubeConfig, setKubeConfig] = useState<string | undefined>()

  const [initialModalOpen, setInitialModalOpen] = useState(false)
  const [portData, setPortData] = useState<
    { remote_port: number; port?: number | string; name?: string | number }[]
  >([])
  const toast = useToast()

  const contextQuery = useQuery<KubeContext[]>(
    ['kube-contexts', kubeConfig],
    () => fetchKubeContexts(kubeConfig),
    {
      enabled: isModalOpen,
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

        setKubeConfig(filePath)
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
    () =>
      invoke<{ name: string }[]>('list_namespaces', {
        contextName: newConfig.context,
        kubeconfig: kubeConfig,
      }),
    {
      initialData: configData?.namespace,
      enabled: !!newConfig.context,
    },
  )

  const serviceQuery = useQuery(
    ['kube-services', newConfig.context, newConfig.namespace],
    () => {
      return invoke<{ name: string }[]>('list_services', {
        contextName: newConfig.context,
        namespace: newConfig.namespace,
        kubeconfig: kubeConfig,
      })
    },
    {
      initialData: configData?.service,
      enabled: !!newConfig.context && !!newConfig.namespace,
    },
  )

  useEffect(() => {
    if (isModalOpen) {
      setInitialModalOpen(true)
    } else {
      setInitialModalOpen(false)
    }
  }, [isModalOpen])

  useEffect(() => {
    if (
      !newConfig.context ||
      !newConfig.namespace ||
      !newConfig.service ||
      !isModalOpen ||
      (isEdit && initialModalOpen)
    ) {
      return
    }

    console.log('Effect triggered with newConfig:', newConfig)
    console.log('Invoking list_service_ports with:', newConfig)
    invoke<{ remote_port: number }[]>('list_service_ports', {
      contextName: newConfig.context,
      namespace: newConfig.namespace,
      serviceName: newConfig.service,
      kubeconfig: kubeConfig,
    })
    .then(ports => {
      console.log('Ports fetched successfully:', ports)
      setPortData(ports)
    })
    .catch(error => {
      console.error('Error fetching service ports:', error)
      toast({
        title: 'Error fetching service ports',
        description: error.message || error.toString(),
        status: 'error',
      })
      setPortData(configData?.ports ?? [])
    })

    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [
    isModalOpen,
    newConfig.context,
    newConfig.namespace,
    newConfig.service,
    toast,
    configData?.ports,
    isEdit,
  ])

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
      setSelectedService(
        newConfig.service
          ? { label: newConfig.service, value: newConfig.service }
          : null,
      )
      setSelectedPort(
        newConfig.remote_port !== undefined
          ? {
            label: newConfig.remote_port.toString(),
            value: newConfig.remote_port,
          }
          : null,
      )
      // Update the kubeConfig state based on the newConfig.kubeconfig value
      setKubeConfig(newConfig.kubeconfig || 'default') // Adjust this line
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
  ])
  useEffect(() => {
    if (!isModalOpen) {
      resetState()
    }
  }, [isModalOpen])

  const resetState = () => {
    setSelectedContext(null)
    setSelectedNamespace(null)
    setSelectedService(null)
    setSelectedPort(null)
    setPortData([])
    setSelectedWorkloadType(null)
    setSelectedProtocol(null)
    setKubeConfig(undefined)
  }

  const handleSelectChange = (
    selectedOption: Option | null,
    { name }: ActionMeta<Option>,
  ) => {
    switch (name) {
    case 'context':
      setSelectedContext(selectedOption)
      if (
        !selectedOption ||
          selectedContext?.value !== selectedOption?.value
      ) {
        setSelectedNamespace(null)
        setSelectedService(null)
        setSelectedPort(null)
        setPortData([])
        handleInputChange({
          target: { name: 'namespace', value: '' },
        } as unknown as React.ChangeEvent<HTMLInputElement>)
        handleInputChange({
          target: { name: 'service', value: '' },
        } as unknown as React.ChangeEvent<HTMLInputElement>)
        handleInputChange({
          target: { name: 'remote_port', value: '' },
        } as unknown as React.ChangeEvent<HTMLInputElement>)
      }
      break
    case 'namespace':
      setSelectedNamespace(selectedOption)
      if (
        !selectedOption ||
          selectedNamespace?.value !== selectedOption?.value
      ) {
        setSelectedService(null)
        setSelectedPort(null)
        setPortData([])
        handleInputChange({
          target: { name: 'service', value: '' },
        } as unknown as React.ChangeEvent<HTMLInputElement>)
        handleInputChange({
          target: { name: 'remote_port', value: '' },
        } as unknown as React.ChangeEvent<HTMLInputElement>)
      }
      break
    case 'service':
      setSelectedService(selectedOption)
      if (
        !selectedOption ||
          selectedService?.value !== selectedOption?.value
      ) {
        setSelectedPort(null)
        setPortData([])
        handleInputChange({
          target: { name: 'remote_port', value: '' },
        } as unknown as React.ChangeEvent<HTMLInputElement>)
      }
      break
    case 'remote_port':
      setSelectedPort(selectedOption)
      break
    case 'workload_type':
      setSelectedWorkloadType(selectedOption)
      break
    case 'protocol':
      setSelectedProtocol(selectedOption)
      break
    }

    handleInputChange({
      target: {
        name: name as string,
        value: selectedOption ? selectedOption.value : '',
      },
    } as unknown as React.ChangeEvent<HTMLInputElement>)
  }

  useEffect(() => {
    if (setNewConfig) {
      // Check if setNewConfig is not undefined
      setNewConfig((prevConfig: Config) => ({
        ...prevConfig,
        kubeconfig: kubeConfig ?? '',
      }))
    }
  }, [kubeConfig, setNewConfig])

  const handleSave = async (event: React.FormEvent) => {
    event.preventDefault()

    const configToSave = {
      ...newConfig,
      kubeconfig: kubeConfig ?? '',
    }

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

  return (
    <Center>
      <Modal isOpen={isModalOpen} onClose={handleCancel}>
        <ModalOverlay bg='transparent' />
        <ModalContent
          mx={5}
          my={5}
          mt={3}
          borderRadius='lg'
          boxShadow='0px 10px 25px 5px rgba(0,0,0,0.5)'
          maxW='27rem'
          maxH='35rem'
        >
          <ModalCloseButton />
          <ModalBody p={2} mt={3}>
            <form onSubmit={handleSave}>
              <FormControl
                display='flex'
                alignItems='center'
                flexWrap='wrap'
                p={2}
              >
                <Box width={{ base: '100%', sm: '50%' }} pl={2}>
                  <FormLabel htmlFor='alias'>Alias</FormLabel>
                  <Input
                    id='alias'
                    type='string'
                    value={newConfig.alias || ''}
                    name='alias'
                    onChange={handleInputChange}
                    size='sm'
                    height='36px'
                    bg={theme.colors.gray[800]}
                    borderColor={theme.colors.gray[700]}
                    _hover={{
                      borderColor: theme.colors.gray[600],
                    }}
                    _placeholder={{
                      color: theme.colors.gray[500],
                    }}
                    color={theme.colors.gray[300]}
                  />

                  <Checkbox
                    mt='0.5'
                    ml='0.5'
                    size='sm'
                    name='domain_enabled'
                    isChecked={newConfig.domain_enabled || false}
                    onChange={e =>
                      handleInputChange({
                        target: {
                          name: e.target.name,
                          value: e.target.checked,
                        },
                      } as unknown as React.ChangeEvent<HTMLInputElement>)
                    }
                  >
                    <Tooltip
                      label='Add a hostfile entry to resolve alias to local address'
                      placement='bottom-end'
                      fontSize='xs'
                      lineHeight='tight'
                    >
                      <Text fontSize='xs'>enable alias as local domain</Text>
                    </Tooltip>
                  </Checkbox>
                </Box>
                <Box width={{ base: '100%', sm: '50%' }} pl={2} mt='-6'>
                  <FormLabel htmlFor='workload_type'>Workload Type</FormLabel>
                  <ReactSelect
                    styles={customStyles}
                    name='workload_type'
                    options={[
                      { label: 'service', value: 'service' },
                      { label: 'proxy', value: 'proxy' },
                    ]}
                    value={selectedWorkloadType}
                    onChange={selectedOption =>
                      handleSelectChange(
                        selectedOption as Option,
                        { name: 'workload_type' } as ActionMeta<Option>,
                      )
                    }
                  />
                </Box>
              </FormControl>

              <FormControl
                display='flex'
                alignItems='center'
                flexWrap='wrap'
                p={2}
              >
                <Box width={{ base: '100%', sm: '50%' }} pl={2}>
                  <FormLabel htmlFor='context'>Context</FormLabel>
                  <ReactSelect
                    styles={customStyles}
                    name='context'
                    options={contextQuery.data?.map(context => ({
                      label: context.name,
                      value: context.name,
                    }))}
                    value={selectedContext}
                    onChange={selectedOption =>
                      handleSelectChange(
                        selectedOption as Option,
                        { name: 'context' } as ActionMeta<Option>,
                      )
                    }
                  />
                </Box>
                <Box width={{ base: '100%', sm: '50%' }} pl={2}>
                  <FormLabel htmlFor='namespace'>Namespace</FormLabel>
                  <ReactSelect
                    styles={customStyles}
                    name='namespace'
                    options={namespaceQuery.data?.map(namespace => ({
                      label: namespace.name,
                      value: namespace.name,
                    }))}
                    value={selectedNamespace}
                    onChange={selectedOption =>
                      handleSelectChange(
                        selectedOption as Option,
                        { name: 'namespace' } as ActionMeta<Option>,
                      )
                    }
                  />
                </Box>
              </FormControl>

              {newConfig.workload_type.startsWith('proxy') ? (
                <>
                  <FormControl
                    display='flex'
                    alignItems='center'
                    flexWrap='wrap'
                    p={2}
                  >
                    <Box width={{ base: '100%', sm: '60%' }} pl={2}>
                      <FormLabel htmlFor='remote_address'>
                        Remote Address
                      </FormLabel>
                      <Input
                        id='remote_address'
                        type='text'
                        height='36px'
                        value={newConfig.remote_address || ''}
                        name='remote_address'
                        onChange={handleInputChange}
                        size='sm'
                        bg={theme.colors.gray[800]}
                        borderColor={theme.colors.gray[700]}
                        _hover={{
                          borderColor: theme.colors.gray[600],
                        }}
                        _placeholder={{
                          color: theme.colors.gray[500],
                        }}
                        color={theme.colors.gray[300]}
                      />
                    </Box>
                    <Box width={{ base: '100%', sm: '40%' }} pl={2}>
                      <FormLabel htmlFor='protocol'>Protocol</FormLabel>
                      <ReactSelect
                        styles={customStyles}
                        name='protocol'
                        options={[
                          { label: 'udp', value: 'udp' },
                          { label: 'tcp', value: 'tcp' },
                        ]}
                        value={selectedProtocol}
                        onChange={selectedOption =>
                          handleSelectChange(
                            selectedOption as Option,
                            { name: 'protocol' } as ActionMeta<Option>,
                          )
                        }
                      />
                    </Box>
                  </FormControl>
                  <FormControl
                    display='flex'
                    alignItems='center'
                    flexWrap='wrap'
                    p={2}
                  >
                    <Box width={{ base: '100%', sm: '50%' }} pl={2}>
                      <FormLabel htmlFor='remote_port'>Target Port</FormLabel>
                      <Input
                        id='remote_port'
                        type='number'
                        height='36px'
                        value={newConfig.remote_port || ''}
                        name='remote_port'
                        onChange={handleInputChange}
                        size='sm'
                        bg={theme.colors.gray[800]}
                        borderColor={theme.colors.gray[700]}
                        _hover={{
                          borderColor: theme.colors.gray[600],
                        }}
                        _placeholder={{
                          color: theme.colors.gray[500],
                        }}
                        color={theme.colors.gray[300]}
                      />
                    </Box>

                    <Box width={{ base: '100%', sm: '50%' }} pl={2}>
                      <FormLabel htmlFor='local_port'>Local Port</FormLabel>
                      <Input
                        id='local_port'
                        type='number'
                        value={newConfig.local_port || ''}
                        name='local_port'
                        height='36px'
                        onChange={handleInputChange}
                        size='sm'
                        bg={theme.colors.gray[800]}
                        borderColor={theme.colors.gray[700]}
                        _hover={{
                          borderColor: theme.colors.gray[600],
                        }}
                        _placeholder={{
                          color: theme.colors.gray[500],
                        }}
                        color={theme.colors.gray[300]}
                      />
                    </Box>
                  </FormControl>
                </>
              ) : (
                <>
                  <FormControl
                    display='flex'
                    alignItems='center'
                    flexWrap='wrap'
                    p={2}
                  >
                    <Box width={{ base: '100%', sm: '50%' }} pl={2}>
                      <FormLabel htmlFor='service'>Service</FormLabel>
                      <ReactSelect
                        styles={customStyles}
                        name='service'
                        options={serviceQuery.data?.map(service => ({
                          label: service.name,
                          value: service.name,
                        }))}
                        value={selectedService}
                        onChange={selectedOption =>
                          handleSelectChange(
                            selectedOption as Option,
                            { name: 'service' } as ActionMeta<Option>,
                          )
                        }
                      />
                    </Box>
                    <Box width={{ base: '100%', sm: '50%' }} pl={2}>
                      <FormLabel htmlFor='protocol'>Protocol</FormLabel>
                      <ReactSelect
                        styles={customStyles}
                        name='protocol'
                        options={[
                          { label: 'udp', value: 'udp' },
                          { label: 'tcp', value: 'tcp' },
                        ]}
                        value={selectedProtocol}
                        onChange={selectedOption =>
                          handleSelectChange(
                            selectedOption as Option,
                            { name: 'protocol' } as ActionMeta<Option>,
                          )
                        }
                      />
                    </Box>
                  </FormControl>
                  <FormControl
                    display='flex'
                    alignItems='center'
                    flexWrap='wrap'
                    p={2}
                  >
                    <Box width={{ base: '100%', sm: '50%' }} pl={2}>
                      <FormLabel htmlFor='remote_port'>Target Port</FormLabel>
                      <ReactSelect
                        styles={customStyles}
                        name='remote_port'
                        options={portData
                        .filter(port => port.port !== undefined)
                        .map(port => ({
                          label: port.port
                            ? port.port.toString() + ' - ' + port.name
                            : '',
                          value: port.port,
                        }))}
                        value={selectedPort}
                        onChange={selectedOption =>
                          handleSelectChange(
                            selectedOption as Option,
                            { name: 'remote_port' } as ActionMeta<Option>,
                          )
                        }
                      />
                    </Box>
                    <Box width={{ base: '100%', sm: '50%' }} pl={2}>
                      <FormLabel htmlFor='local_port'>Local Port</FormLabel>
                      <Input
                        id='local_port'
                        type='number'
                        height='36px'
                        value={newConfig.local_port || ''}
                        name='local_port'
                        onChange={handleInputChange}
                        size='sm'
                        bg={theme.colors.gray[800]}
                        borderColor={theme.colors.gray[700]}
                        _hover={{
                          borderColor: theme.colors.gray[600],
                        }}
                        _placeholder={{
                          color: theme.colors.gray[500],
                        }}
                        color={theme.colors.gray[300]}
                      />
                    </Box>
                  </FormControl>
                </>
              )}
              <Box width={{ base: '100%', sm: '52%' }} pl={2}>
                <FormControl display='flex' flexDirection='column' p={2}>
                  <FormLabel htmlFor='local_address'>
                    <Checkbox
                      mt='1.5'
                      mr='1'
                      size='sm'
                      isChecked={isChecked}
                      onChange={handleCheckboxChange}
                    >
                      Custom Local Address
                    </Checkbox>
                  </FormLabel>
                  <Input
                    id='local_address'
                    isDisabled={!isChecked}
                    value={newConfig.local_address || '127.0.0.1'}
                    type='text'
                    height='36px'
                    name='local_address'
                    onChange={handleInputChange}
                    size='sm'
                    bg='gray.800'
                    borderColor='gray.700'
                    _hover={{
                      borderColor: 'gray.600',
                    }}
                    _placeholder={{
                      color: 'gray.500',
                    }}
                    color='gray.300'
                  />
                </FormControl>
              </Box>
              <ModalFooter justifyContent='space-between' p={2} mt='3'>
                <Box display={{ base: 'block', sm: 'flex' }} width='100%'>
                  <Button
                    variant='outline'
                    onClick={handleSetKubeConfig}
                    size='xs'
                    mr={{ base: 0, sm: 3 }}
                    mb={{ base: 2, sm: 0 }}
                    width={{ base: '100%', sm: 'auto' }}
                  >
                    {kubeConfig && kubeConfig !== 'default' ? (
                      <Text fontSize='xs'>{kubeConfig}</Text> // Display the kubeConfig path
                    ) : (
                      <Text fontSize='xs'>Set Kubeconfig</Text> // Display 'Set Kubeconfig'
                    )}
                  </Button>
                  <Box
                    flex='1'
                    display='flex'
                    justifyContent={{ base: 'flex-start', sm: 'flex-end' }}
                  >
                    <Button
                      variant='outline'
                      onClick={handleCancel}
                      size='xs'
                      mr={3}
                    >
                      Cancel
                    </Button>
                    <Button
                      type='submit'
                      colorScheme='blue'
                      size='xs'
                      onClick={handleSave}
                      isDisabled={!isFormValid}
                    >
                      {isEdit ? 'Save Changes' : 'Add Config'}
                    </Button>
                  </Box>
                </Box>
              </ModalFooter>
            </form>
          </ModalBody>
        </ModalContent>
      </Modal>
    </Center>
  )
}

export default AddConfigModal
