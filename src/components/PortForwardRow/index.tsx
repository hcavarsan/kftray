import React from 'react'

import {
  AlertDialog,
  AlertDialogBody,
  AlertDialogContent,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogOverlay,
  Button,
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
}) => {
  const { isOpen, onOpen, onClose } = useDisclosure()
  const textColor = useColorModeValue('gray.100', 'gray.100')
  const cancelRef = React.useRef<HTMLButtonElement>(null)
  const [isToggling, setIsToggling] = useBoolean(false)

  const togglePortForwarding = async (isChecked: boolean) => {
    setIsToggling.on()
    try {
      if (isChecked) {
        await invoke('start_port_forward', { configs: [config] })
        updateConfigRunningState(config.id, true)
      } else {
        await invoke('stop_port_forward', { serviceName: config.service })
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

  return (
    <>
      <Tr key={config.id}>
        <Td width='20%' color={textColor}>
          {config.service}
        </Td>
        <Td width='20%' color={textColor}>
          {config.namespace}
        </Td>
        <Td width='20%' color={textColor}>
          {config.local_port}
        </Td>
        <Td width='20%' color={config.isRunning ? 'green.500' : 'red.500'}>
          <HStack position='relative' ml='5px'>
            <Switch
              isChecked={config.isRunning}
              colorScheme='facebook'
              size='md'
              onChange={e => togglePortForwarding(e.target.checked)}
            />
          </HStack>
        </Td>
        <Td width='20%'>
          <HStack spacing='-1' mr='10px'>
            <IconButton
              size='sm'
              aria-label='Edit configuration'
              icon={<FontAwesomeIcon icon={faPen} />}
              onClick={() => handleEditConfig(config.id)}
              variant='ghost'
            />
            <IconButton
              size='sm'
              aria-label='Delete configuration'
              icon={<FontAwesomeIcon icon={faTrash} />}
              onClick={() => {
                setIsAlertOpen(true),
                handleDeleteClick(),
                handleDeleteConfig(config.id)
              }}
              variant='ghost'
            />
          </HStack>
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
