import {
  AlertDialog,
  AlertDialogBody,
  AlertDialogContent,
  AlertDialogFooter,
  AlertDialogHeader,
  Button,
} from "@chakra-ui/react"

import {
  Tr,
  Td,
  HStack,
  Icon,
  IconButton,
  useColorModeValue,
} from "@chakra-ui/react"

import { FontAwesomeIcon } from "@fortawesome/react-fontawesome"
import { faTrash, faPen } from "@fortawesome/free-solid-svg-icons"
import React, { RefObject } from "react"

const StatusIcon: React.FC<{ isRunning: boolean }> = ({ isRunning }) => {
  return (
    <Icon viewBox='0 0 200 200' color={isRunning ? "green.500" : "red.500"}>
      <path
        fill='currentColor'
        d='M 100, 100 m -75, 0 a 75,75 0 1,0 150,0 a 75,75 0 1,0 -150,0'
      />
    </Icon>
  )
}
interface DeleteConfirmationProps {
  isAlertOpen: boolean
  setIsAlertOpen: (isOpen: boolean) => void
  cancelRef: RefObject<HTMLButtonElement>
  confirmDeleteConfig: () => void
}

const DeleteConfirmation: React.FC<DeleteConfirmationProps> = props => {
  const { isAlertOpen, setIsAlertOpen, cancelRef, confirmDeleteConfig } = props

  return (
    <AlertDialog
      isOpen={isAlertOpen}
      onClose={() => setIsAlertOpen(false)}
      leastDestructiveRef={cancelRef}
    >
      <AlertDialogContent>
        <AlertDialogHeader fontSize='md' fontWeight='bold'>
          Delete Configuration
        </AlertDialogHeader>

        <AlertDialogBody>
          Are you sure? This action cannot be undone.
        </AlertDialogBody>

        <AlertDialogFooter>
          <Button onClick={() => setIsAlertOpen(false)}>Cancel</Button>
          <Button colorScheme='red' onClick={confirmDeleteConfig} ml={3}>
            Yes
          </Button>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  )
}

interface PortFowardProps {
  config: {
    id: number
    service: string
    context: string
    namespace: string
    local_port: number
    isRunning: boolean
    cancelRef: RefObject<HTMLButtonElement>
  }
  confirmDeleteConfig: () => void
  handleDeleteConfig: (id: number) => void
  handleEditConfig: (id: number) => void
  isAlertOpen: boolean
  setIsAlertOpen: (isOpen: boolean) => void
}

const PortFoward: React.FC<PortFowardProps> = props => {
  const {
    config,
    confirmDeleteConfig,
    handleDeleteConfig,
    handleEditConfig,
    isAlertOpen,
    setIsAlertOpen,
  } = props

  const textColor = useColorModeValue("gray.100", "gray.100")

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
        <Td width='5%' color={config.isRunning ? "green.100" : "red.100"}>
          <StatusIcon isRunning={config.isRunning} />
        </Td>
        <Td width='10%'>
          <HStack spacing='-1' mr='-10px' ml='15px'>
            <IconButton
              aria-label='Edit config'
              icon={<FontAwesomeIcon icon={faPen} />}
              size='sm'
              onClick={() => handleEditConfig(config.id)}
              variant='ghost'
            />
            <IconButton
              aria-label='Delete config'
              size='sm'
              icon={<FontAwesomeIcon icon={faTrash} />}
              onClick={() => handleDeleteConfig(config.id)}
              variant='ghost'
            />
            <DeleteConfirmation
              isAlertOpen={isAlertOpen}
              setIsAlertOpen={setIsAlertOpen}
              cancelRef={config.cancelRef}
              confirmDeleteConfig={confirmDeleteConfig}
            />
          </HStack>
        </Td>
      </Tr>
    </>
  )
}

export { PortFoward }
