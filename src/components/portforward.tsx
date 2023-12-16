import {
    AlertDialog,
    AlertDialogBody,
    AlertDialogContent,
    AlertDialogFooter,
    AlertDialogHeader,
    Button,
    Flex
} from "@chakra-ui/react"

import {
    Tr,
    Td,
    HStack,
    Icon,
    IconButton,
    useColorModeValue,
} from "@chakra-ui/react"

import { FontAwesomeIcon } from '@fortawesome/react-fontawesome'
import { faTrash, faPen } from "@fortawesome/free-solid-svg-icons"

const StatusIcon: React.FC<{ isRunning: boolean }> = ({ isRunning }) => {
    return (
      <Icon viewBox="0 0 200 200" color={isRunning ? "green.500" : "red.500"}>
        <path
          fill="currentColor"
          d="M 100, 100 m -75, 0 a 75,75 0 1,0 150,0 a 75,75 0 1,0 -150,0"
        />
      </Icon>
    )
  }

const DeleteConfirmation: React.FC = (props) => {
    const { isAlertOpen, setIsAlertOpen, cancelRef, confirmDeleteConfig } = props

    return (
        <AlertDialog
            isOpen={isAlertOpen}
            onClose={() => setIsAlertOpen(false)}
            leastDestructiveRef={cancelRef}
        >
            <AlertDialogContent>
                <AlertDialogHeader fontSize="md" fontWeight="bold">
                Delete Configuration
                </AlertDialogHeader>

                <AlertDialogBody>
                Are you sure? This action cannot be undone.
                </AlertDialogBody>

                <AlertDialogFooter>
                <Button onClick={() => setIsAlertOpen(false)}>
                    Cancel
                </Button>
                <Button
                    colorScheme="red"
                    onClick={confirmDeleteConfig}
                    ml={3}
                >
                    Yes
                </Button>
                </AlertDialogFooter>
            </AlertDialogContent>
        </AlertDialog>
    )
}

const PortFoward = (props) => {
    const { 
        config,
        confirmDeleteConfig,
        handleDeleteConfig,
        handleEditConfig,
        isAlertOpen,
        setIsAlertOpen
    } = props
    const textColor = useColorModeValue("gray.100", "gray.100")

    return (
        <>
        <Tr key={config.id} color={textColor}>
            <Td>{config.service}</Td>
            <Td>{config.context}</Td>
            <Td>{config.namespace}</Td>
            <Td>{config.local_port}</Td>
            <Td color={config.isRunning ? "green.100" : "red.100"}>
                <StatusIcon isRunning={config.isRunning} />
            </Td>
            <Td>
                <Flex direction="row">
                <IconButton
                    aria-label="Edit config"
                    icon={<FontAwesomeIcon icon={faPen} />}
                    size="sm"
                    onClick={() => handleEditConfig(config.id)}
                    variant="ghost"
                />
                <IconButton
                    aria-label="Delete config"
                    size="sm"
                    icon={<FontAwesomeIcon icon={faTrash} />}
                    onClick={() => handleDeleteConfig(config.id)}
                    variant="ghost"
                />
                <DeleteConfirmation
                    isAlertOpen={isAlertOpen}
                    setIsAlertOpen={setIsAlertOpen}
                    cancelRef={config.cancelRef}
                    confirmDeleteConfig={confirmDeleteConfig}
                />
                </Flex>
            </Td>
            </Tr>
        </>
    )
}

export {
    PortFoward
}