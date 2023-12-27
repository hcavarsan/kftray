import React, { useEffect, useState } from 'react'
import { useQuery, useQueryClient } from 'react-query'
import ReactSelect, { ActionMeta, StylesConfig } from 'react-select'

import {
  Button,
  Center,
  FormControl,
  FormLabel,
  Input,
  Modal,
  ModalBody,
  ModalCloseButton,
  ModalContent,
  ModalFooter,
  ModalOverlay,
  useToast,
} from '@chakra-ui/react'
import { invoke } from '@tauri-apps/api/tauri'

import theme from '../../assets/theme'
import { ConfigProps, Status } from '../../types'

interface Namespace {
  namespace: string
  name: string
}
interface Context {
  value: string
  label: string
}

interface Service {
  name: string
  service: string
}

interface Port {
  remote_port: number
  name?: string
  port?: number
}

interface KubeContext {
  name: string
}

interface CustomConfigProps extends ConfigProps {
  configData?: {
    context?: KubeContext[]
    namespace?: Namespace[]
    service?: Service[]
    port: number
    name?: string
    remote_port?: number
    ports?: Port[]
  }
}
type Option = { name: string; value: string | number; label: string }
const AddConfigModal: React.FC<CustomConfigProps> = ({
  isModalOpen,
  closeModal,
  newConfig,
  handleInputChange,
  handleSaveConfig,
  isEdit,
  configData,
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

  const queryClient = useQueryClient()
  const customStyles: StylesConfig = {
    control: provided => ({
      ...provided,
      background: theme.colors.gray[800],
      borderColor: theme.colors.gray[700],
    }),
    menu: provided => ({
      ...provided,
      background: theme.colors.gray[800],
    }),
    option: (provided, state) => ({
      ...provided,
      color: state.isSelected ? theme.colors.white : theme.colors.gray[300],
      background: state.isSelected ? theme.colors.gray[600] : 'none',
      cursor: 'pointer',
      ':active': {
        ...provided[':active'],
        background: theme.colors.gray[500],
      },
      ':hover': {
        ...provided[':hover'],
        background: theme.colors.gray[700],
        color: theme.colors.white,
      },
    }),
    singleValue: provided => ({
      ...provided,
      color: theme.colors.gray[300],
    }),
    input: provided => ({
      ...provided,
      color: theme.colors.gray[300],
    }),
    placeholder: provided => ({
      ...provided,
      color: theme.colors.gray[500],
    }),
  }

  const [portData, setPortData] = useState<
    { remote_port: number; port?: number | string; name?: string | number }[]
  >([])
  const toast = useToast()

  const contextQuery = useQuery<KubeContext[]>(
    'kube-contexts',
    () => invoke('list_kube_contexts') as Promise<KubeContext[]>,
  )

  const namespaceQuery = useQuery(
    ['kube-namespaces', newConfig.context],
    () =>
      invoke<{ name: string }[]>('list_namespaces', {
        contextName: newConfig.context,
      }),
    {
      initialData: configData?.namespace,
      enabled: !!newConfig.context,
    },
  )

  const serviceQuery = useQuery(
    ['kube-services', newConfig.context, newConfig.namespace],
    () =>
      invoke<{ name: string }[]>('list_services', {
        contextName: newConfig.context,
        namespace: newConfig.namespace,
      }),
    {
      initialData: configData?.service,
      enabled: !!newConfig.context && !!newConfig.namespace,
    },
  )

  useEffect(() => {
    if (newConfig.context && newConfig.namespace && newConfig.service) {
      invoke<{ remote_port: number }[]>('list_service_ports', {
        contextName: newConfig.context,
        namespace: newConfig.namespace,
        serviceName: newConfig.service,
      })
      .then(ports => {
        setPortData(ports)
      })
      .catch(error => {
        toast({
          title: 'Error fetching service ports',
          description: error,
          status: 'error',
        })
        setPortData(configData?.ports || [])
      })
    } else {
      setPortData(configData?.ports || [])
    }
  }, [
    newConfig.context,
    newConfig.namespace,
    newConfig.service,
    toast,
    configData,
  ])

  useEffect(() => {
    if (isEdit && isModalOpen) {
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
    }
  }, [
    isEdit,
    isModalOpen,
    newConfig.context,
    newConfig.namespace,
    newConfig.service,
    newConfig.remote_port,
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
    }

    handleInputChange({
      target: {
        name: name as string,
        value: selectedOption ? selectedOption.value : '',
      },
    } as unknown as React.ChangeEvent<HTMLInputElement>)
  }
  const handleSave = (event: React.FormEvent<Element>) => {
    event.preventDefault()
    handleSaveConfig(event)
    if (!isEdit) {
      resetState()
    }
    closeModal()
  }

  const handleCancel = () => {
    closeModal()
    resetState()
  }

  const onSubmit = isEdit
    ? handleSaveConfig
    : (event: React.FormEvent<Element>) => {
      event.preventDefault()
      handleSaveConfig(event)
    }

  return (
    <Center>
      <Modal
        isOpen={isModalOpen}
        onClose={handleCancel}
        size='xs'
      >
        <ModalOverlay bg='transparent' />
        <ModalContent mx={5} my={5} mt={8} w='auto' h='auto'>
          <ModalCloseButton />
          <ModalBody p={2} mt={3}>
            <form onSubmit={handleSave}>
              <FormControl>
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
              </FormControl>

              <FormControl mt={4}>
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
              </FormControl>

              <FormControl mt={4}>
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
              </FormControl>

              <FormControl mt={4}>
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
              </FormControl>

              <FormControl mt={2}>
                <FormLabel htmlFor='local_port'>Local Port</FormLabel>
                <Input
                  id='local_port'
                  type='number'
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
              </FormControl>

              <ModalFooter justifyContent='flex-end' p={2} mt={5}>
                <Button variant='outline' onClick={handleCancel} size='xs'>
                  Cancel
                </Button>
                <Button
                  type='submit'
                  colorScheme='blue'
                  size='xs'
                  ml={3}
                  onClick={handleSave}
                >
                  {isEdit ? 'Save Changes' : 'Add Config'}
                </Button>
              </ModalFooter>
            </form>
          </ModalBody>
        </ModalContent>
      </Modal>
    </Center>
  )
}

export default AddConfigModal
