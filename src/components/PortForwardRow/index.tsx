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
  HStack,
  IconButton,
  Portal,
  Switch,
  Td,
  Tr,
  useBoolean,
  useColorModeValue,
  useDisclosure,
} from '@chakra-ui/react'
import { faPen, faTrash } from '@fortawesome/free-solid-svg-icons'
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome'
import { invoke } from '@tauri-apps/api/tauri'

import { PortForwardRowProps, Status } from '../../types'

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
  const [isToggling, setIsToggling] = useBoolean(false)

  const togglePortForwarding = async (isChecked: boolean) => {
    setIsToggling.on()
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
        config.remote_address +
        ' ' +
        config.protocol,
    )
    try {
      if (isChecked) {
        if (config.workload_type === 'service') {
          await invoke('start_port_forward', { configs: [config] })
        } else if (config.workload_type === 'proxy') {
          await invoke('deploy_and_forward_pod', { configs: [config] })
        } else {
          throw new Error(`Unsupported workload type: ${config.workload_type}`)
        }
        updateConfigRunningState(config.id, true)
      } else {
        // Stopping port forwarding logic
        if (config.workload_type === 'service') {
          await invoke('stop_port_forward', {
            serviceName: config.service,
            configId: config.id.toString(),
          })
        } else if (config.workload_type === 'proxy') {
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
    } catch (error) {
      console.error('Error toggling port-forwarding:', error)
      updateConfigRunningState(config.id, false)
    } finally {
      setIsToggling.off()
    }
  }

  const handleDeleteClick = () => {
    onOpen()
  }
  const fontFamily = '\'Inter\', sans-serif'

  return (
    <>
      <Tr key={config.id}>
        {showContext && <Td width='10%'>{config.context}</Td>}
        <Td width='20%' color={textColor} fontFamily={fontFamily}>
          {config.workload_type === 'proxy'
            ? config.remote_address
            : config.service}
        </Td>
        <Td width='20%' color={textColor} fontFamily={fontFamily}>
          {config.namespace}
        </Td>
        <Td width='15%' color={textColor} fontFamily={fontFamily}>
          {config.local_port}
        </Td>
        <Td width='15%'>
          <Switch
            colorScheme='green'
            isChecked={config.isRunning}
            size='sm'
            onChange={e => togglePortForwarding(e.target.checked)}
          />
        </Td>
        <Td textAlign='center' width='20%'>
          <Flex justifyContent='center'>
            <IconButton
              size='xs'
              aria-label='Edit configuration'
              icon={
                <FontAwesomeIcon icon={faPen} style={{ fontSize: '10px' }} />
              }
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
                setIsAlertOpen(true),
                handleDeleteClick(),
                handleDeleteConfig(config.id)
              }}
              variant='ghost'
            />
          </Flex>
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
