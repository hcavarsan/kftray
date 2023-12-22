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
  Icon,
  IconButton,
  Td,
  Tr,
  useColorModeValue,
  useDisclosure,
} from '@chakra-ui/react'
import { faPen, faTrash } from '@fortawesome/free-solid-svg-icons'
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome'

import { PortForwardRowProps } from '../../types'

const StatusIcon: React.FC<{ isRunning: boolean }> = ({ isRunning }) => {
  return (
    <Icon viewBox='0 0 200 200' color={isRunning ? 'green.500' : 'red.500'}>
      <path
        fill='currentColor'
        d='M 100, 100 m -75, 0 a 75,75 0 1,0 150,0 a 75,75 0 1,0 -150,0'
      />
    </Icon>
  )
}

const PortForwardRow: React.FC<PortForwardRowProps> = ({
  config,
  confirmDeleteConfig,
  handleDeleteConfig,
  handleEditConfig,
  isAlertOpen,
  setIsAlertOpen,
}) => {
  const { isOpen, onOpen, onClose } = useDisclosure()
  const textColor = useColorModeValue('gray.100', 'gray.100')
  const cancelRef = React.useRef<HTMLElement>(null)


  
  return (
    <>
      <Tr key={config.id}>
        <Td width='20%' color={textColor}>
          {config.service}
        </Td>
        <Td width='20%' color={textColor}>
          {config.context}
        </Td>
        <Td width='20%' color={textColor}>
          {config.namespace}
        </Td>
        <Td width='20%' color={textColor}>
          {config.local_port}
        </Td>
        <Td width='5%' color={config.isRunning ? 'green.500' : 'red.500'}>
          <StatusIcon isRunning={config.isRunning} />
        </Td>
        <Td width='10%'>
          <HStack spacing='-1' mr='-10px' ml='15px'>
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
                setIsAlertOpen(true)
                onOpen()
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
                <Button ref={undefined} onClick={onClose}>
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
