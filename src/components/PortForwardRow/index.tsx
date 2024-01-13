import React from 'react'

import {
  AlertDialog,
  AlertDialogBody,
  AlertDialogContent,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogOverlay,
  Box,
  Button,
  Flex,
  IconButton,
  Switch,
  Td,
  Tooltip,
  Tr,
  useColorModeValue,
  useDisclosure,
} from '@chakra-ui/react'
import { faInfoCircle, faPen, faTrash } from '@fortawesome/free-solid-svg-icons'
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome'
import { invoke } from '@tauri-apps/api/tauri'

import { PortForwardRowProps } from '../../types'

const PortForwardRow: React.FC<PortForwardRowProps> = ({
  config,
  confirmDeleteConfig,
  handleDeleteConfig,
  handleEditConfig,
  setIsAlertOpen,
  isAlertOpen,
  updateConfigRunningState,
  showContext = false,
}) => {
  const { isOpen, onOpen, onClose } = useDisclosure()
  const textColor = useColorModeValue('gray.100', 'gray.100')
  const cancelRef = React.useRef<HTMLButtonElement>(null)

  const startPortForwarding = async () => {
    if (config.workload_type === 'service' && config.protocol === 'tcp') {
      await invoke('start_port_forward', { configs: [config] })
    } else if (
      config.workload_type.startsWith('proxy') ||
      (config.workload_type === 'service' && config.protocol === 'udp')
    ) {
      await invoke('deploy_and_forward_pod', { configs: [config] })
    } else {
      throw new Error(`Unsupported workload type: ${config.workload_type}`)
    }
    updateConfigRunningState(config.id, true)
  }

  const stopPortForwarding = async () => {
    if (config.workload_type === 'service' && config.protocol === 'tcp') {
      await invoke('stop_port_forward', {
        serviceName: config.service,
        configId: config.id.toString(),
      })
    } else if (
      config.workload_type.startsWith('proxy') ||
      (config.workload_type === 'service' && config.protocol === 'udp')
    ) {
      await invoke('stop_proxy_forward', {
        configId: config.id.toString(),
        namespace: config.namespace,
        serviceName: config.service,
        localPort: config.local_port,
        remoteAddress: config.remote_address,
        protocol: 'tcp',
      })
    } else {
      throw new Error(`Unsupported workload type: ${config.workload_type}`)
    }
    updateConfigRunningState(config.id, false)
  }

  const togglePortForwarding = async (isChecked: boolean) => {
    console.log(
      'togglePortForwarding' +
        config.workload_type +
        ' ' +
        config.service +
        ' ' +
        config.id.toString() +
        ' ' +
        config.namespace +
        ' ' +
        config.local_port +
        ' ' +
        config.remote_address,
      ' ' + config.protocol,
    )
    try {
      if (isChecked) {
        await startPortForwarding()
      } else {
        await stopPortForwarding()
      }
    } catch (error) {
      console.error('Error toggling port-forwarding:', error)
      updateConfigRunningState(config.id, false)
    } finally {
      console.log('togglePortForwarding finally')
    }
  }
  const handleDeleteClick = () => {
    onOpen()
  }

  const infoIcon = (
    <FontAwesomeIcon icon={faInfoCircle} style={{ fontSize: '10px' }} />
  )

  const tooltipLabel = (
    <>
      <Box as='span' fontWeight='semibold'>
        Workload Type:
      </Box>{' '}
      {config.workload_type.startsWith('proxy')
        ? config.workload_type
        : 'Service'}
      <br />
      <Box as='span' fontWeight='semibold'>
        {config.workload_type.startsWith('proxy')
          ? 'Remote Address:'
          : 'Service:'}
      </Box>{' '}
      {config.workload_type.startsWith('proxy')
        ? config.remote_address
        : config.service}
      <br />
      <Box as='span' fontWeight='semibold'>
        Context:
      </Box>{' '}
      {config.context}
      <br />
      <Box as='span' fontWeight='semibold'>
        Namespace:
      </Box>{' '}
      {config.namespace}
      <br />
      <Box as='span' fontWeight='semibold'>
        Target Port:
      </Box>{' '}
      {config.remote_port}
      <br />
      <Box as='span' fontWeight='semibold'>
        Local Port:
      </Box>{' '}
      {config.local_port}
      <br />
      <Box as='span' fontWeight='semibold'>
        Protocol:
      </Box>{' '}
      {config.protocol}
      <br />
    </>
  )

  const fontFamily = '"Inter", sans-serif'

  return (
    <>
      <Tr key={config.id}>
        {showContext && <Td>{config.context}</Td>}
        <Td color={textColor} fontFamily={fontFamily} width='40%'>
          {config.alias}
          <Tooltip
            hasArrow
            label={tooltipLabel}
            placement='right'
            bg={useColorModeValue('white', 'gray.300')}
            p={2}
          >
            <span>
              <IconButton
                size='xs'
                aria-label='Info configuration'
                icon={infoIcon}
                variant='ghost'
              />
            </span>
          </Tooltip>
        </Td>
        <Td color={textColor} fontFamily={fontFamily}>
          {config.local_port}
        </Td>
        <Td>
          <Flex alignItems='center'>
            <Switch
              colorScheme='green'
              isChecked={config.isRunning}
              size='sm'
              onChange={e => togglePortForwarding(e.target.checked)}
            />
          </Flex>
        </Td>
        <Td>
          <IconButton
            size='xs'
            aria-label='Edit configuration'
            icon={<FontAwesomeIcon icon={faPen} style={{ fontSize: '10px' }} />}
            onClick={() => handleEditConfig(config.id)}
            variant='ghost'
          />
          <IconButton
            size='xs'
            aria-label='Delete configuration'
            icon={
              <FontAwesomeIcon icon={faTrash} style={{ fontSize: '10px' }} />
            }
            onClick={() => {
              setIsAlertOpen(true)
              handleDeleteClick()
              handleDeleteConfig(config.id)
            }}
            variant='ghost'
          />
        </Td>
      </Tr>
      {isAlertOpen && (
        <AlertDialog
          isOpen={isOpen}
          leastDestructiveRef={cancelRef}
          onClose={onClose}
        >
          <AlertDialogOverlay bg='transparent'>
            <AlertDialogContent>
              <AlertDialogHeader fontSize='lg' fontWeight='bold'>
                Delete Configuration
              </AlertDialogHeader>
              <AlertDialogBody>
                {'Are you sure? You can\'t undo this action afterwards.'}
              </AlertDialogBody>
              <AlertDialogFooter>
                <Button ref={cancelRef} onClick={onClose}>
                  Cancel
                </Button>
                <Button colorScheme='red' onClick={confirmDeleteConfig} ml={3}>
                  Delete
                </Button>
              </AlertDialogFooter>
            </AlertDialogContent>
          </AlertDialogOverlay>
        </AlertDialog>
      )}
    </>
  )
}

export default PortForwardRow
