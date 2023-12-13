// App.tsx
import React, { useEffect, useState } from "react"
import {
  AlertDialog,
  AlertDialogBody,
  AlertDialogContent,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogOverlay,
  Button,
  Center,
  FormControl,
  FormLabel,
  Heading,
  HStack,
  Icon,
  IconButton,
  Image,
  Input,
  Modal,
  ModalBody,
  ModalCloseButton,
  ModalContent,
  ModalFooter,
  Stack,
  Table,
  Tbody,
  Td,
  Th,
  Thead,
  Tr,
  useColorModeValue,
  VStack,
} from "@chakra-ui/react"
import { sendNotification } from "@tauri-apps/api/notification"
import { invoke } from "@tauri-apps/api/tauri"
import { MdAdd, MdClose, MdDelete, MdEdit, MdRefresh } from "react-icons/md"

import logo from "./logo.png"

interface Response {
  id: number
  service: string
  context: string
  local_port: string
  status: number
  namespace: string
  remote_port: string
  stdout: string
  stderr: string
}

interface Config {
  id: number
  service: string
  namespace: string
  local_port: number
  remote_port: number
  context: string
}

interface Status {
  id: number
  service: string
  context: string
  local_port: string
  isRunning: boolean
  namespace: string
  remote_port: string
}

const App: React.FC = () => {
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

  const [isModalOpen, setIsModalOpen] = useState(false)
  const [isEdit, setIsEdit] = useState(false)
  const [newConfig, setNewConfig] = useState({
    id: 0,
    service: "",
    context: "",
    local_port: "",
    remote_port: "",
    namespace: "",
  })
  const openModal = () => {
    setNewConfig({
      id: 0,
      service: "",
      context: "",
      local_port: "",
      remote_port: "",
      namespace: "",
    })
    setIsEdit(false) // Reset the isEdit state for a new configuration
    setIsModalOpen(true)
  }
  const closeModal = () => {
    setIsModalOpen(false)
    setIsEdit(false) // Reset isEdit when the modal is closed
  }
  const handleInputChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const { name, value } = e.target
    setNewConfig((prev) => ({ ...prev, [name]: value }))
  }
  const cancelRef = React.useRef<HTMLElement>(null)
  const [isInitiating, setIsInitiating] = useState(false)
  const [isStopping, setIsStopping] = useState(false)
  const [isPortForwarding, setIsPortForwarding] = useState(false)
  const [configs, setConfigs] = useState<Status[]>([])
  const [isAlertOpen, setIsAlertOpen] = useState(false)
  const [configToDelete, setConfigToDelete] = useState<number | undefined>(
    undefined
  )

  useEffect(() => {
    const fetchConfigs = async () => {
      try {
        const configsResponse: Status[] = await invoke("get_configs")
        setConfigs(
          configsResponse.map((config) => ({
            ...config,
            // Since we don't know if they are running initially, set them all to false
            isRunning: false,
          }))
        )
      } catch (error) {
        console.error("Failed to fetch configs:", error)
        // Handle error appropriately
      }
    }

    fetchConfigs()
    // You might want to set the initial window size here as well
  }, [])
  const handleEditConfig = async (id: number) => {
    try {
      const configToEdit: Config = await invoke("get_config", { id })
      setNewConfig({
        // populate the state with the fetched config
        id: configToEdit.id,
        service: configToEdit.service,
        namespace: configToEdit.namespace,
        local_port: configToEdit.local_port.toString(),
        remote_port: configToEdit.remote_port.toString(),
        context: configToEdit.context,
      })
      setIsEdit(true) // Set isEdit to true because we are editing
      setIsModalOpen(true)
    } catch (error) {
      console.error(
        `Failed to fetch the config for editing with id ${id}:`,
        error
      )
      // Handle the error...
    }
  }
  const handleEditSubmit = async (e: React.FormEvent) => {
    e.preventDefault() // Prevent the default form submit action

    // Check if the port numbers are within the correct range
    const local_port_number = parseInt(newConfig.local_port, 10)
    const remote_port_number = parseInt(newConfig.remote_port, 10)

    if (local_port_number < 0 || local_port_number > 65535) {
      // Handle error: local port number is out of range for a u16
      return
    }

    if (remote_port_number < 0 || remote_port_number > 65535) {
      // Handle error: remote port number is out of range for a u16
      return
    }
    try {
      // Construct the edited config object
      const editedConfig = {
        id: newConfig.id, // Include the id for updating the existing record
        service: newConfig.service,
        context: newConfig.context,
        local_port: local_port_number,
        remote_port: remote_port_number,
        namespace: newConfig.namespace,
      }

      await invoke("update_config", { config: editedConfig })

      // Fetch the updated configurations
      const updatedConfigs: Status[] = await invoke("get_configs")
      setConfigs(updatedConfigs)

      // Show success notification
      await sendNotification({
        title: "Success",
        body: "Configuration updated successfully.",
        icon: "success",
      })

      closeModal() // Close the modal after successful update
    } catch (error) {
      console.error("Failed to update config:", error)
      // Handle errors
      await sendNotification({
        title: "Error",
        body: "Failed to update configuration.",
        icon: "error",
      })
    }
  }
  const handleSaveConfig = async (e: React.FormEvent) => {
    e.preventDefault() // Prevent the default form submit action

    // Parse and validate port numbers
    const local_port_number = parseInt(newConfig.local_port, 10)
    const remote_port_number = parseInt(newConfig.remote_port, 10)

    if (
      isNaN(local_port_number) ||
      local_port_number < 0 ||
      local_port_number > 65535
    ) {
      await sendNotification({
        title: "Error",
        body: "Local port number is out of range for a u16.",
        icon: "error",
      })
      return
    }

    if (
      isNaN(remote_port_number) ||
      remote_port_number < 0 ||
      remote_port_number > 65535
    ) {
      await sendNotification({
        title: "Error",
        body: "Remote port number is out of range for a u16.",
        icon: "error",
      })
      return
    }

    // Prepare the config object for saving
    const configToSave = {
      // Include ID only for updates, not for new config
      id: isEdit ? newConfig.id : undefined,
      service: newConfig.service,
      context: newConfig.context,
      local_port: local_port_number,
      remote_port: remote_port_number,
      namespace: newConfig.namespace,
    }

    try {
      // Check if we're adding a new config or updating an existing one
      if (isEdit) {
        // Update existing config
        await invoke("update_config", { config: configToSave })
      } else {
        // Insert new config
        await invoke("insert_config", { config: configToSave })
      }

      // Fetch and update the list of configurations
      const updatedConfigs: Status[] = await invoke("get_configs")
      setConfigs(updatedConfigs)

      // Show a success notification to the user
      await sendNotification({
        title: "Success",
        body: `Configuration ${isEdit ? "updated" : "added"} successfully.`,
        icon: "success",
      })

      // Close the modal after successful insert/update
      closeModal()
    } catch (error) {
      console.error(`Failed to ${isEdit ? "update" : "insert"} config:`, error)

      // Handle errors, such as showing an error notification
      await sendNotification({
        title: "Error",
        body: `Failed to ${
          isEdit ? "update" : "add"
        } configuration. Error: ${error}`,
        icon: "error",
      })
    }
  }

  const initiatePortForwarding = async () => {
    setIsInitiating(true)
    try {
      const configsToSend = configs.map((config) => ({
        // Remove the id property if it's not expected by your command
        // Transform local_port and remote_port to the correct type if needed
        ...config,
        local_port: parseInt(config.local_port, 10),
        remote_port: parseInt(config.remote_port, 10),
      }))

      const responses: Response[] = await invoke("start_port_forward", {
        configs: configsToSend,
      })

      // Update each config with its new running status, depending on the response status.
      const updatedConfigs = configs.map((config) => {
        const relatedResponse = responses.find((res) => res.id === config.id)
        return {
          ...config,
          isRunning: relatedResponse ? relatedResponse.status === 0 : false,
        }
      })

      setConfigs(updatedConfigs)
      setIsPortForwarding(true)
    } catch (error) {
      console.error(
        "An error occurred while initiating port forwarding:",
        error
      )
    } finally {
      setIsInitiating(false)
    }
  }

  const handleDeleteConfig = (id?: number) => {
    setConfigToDelete(id)
    setIsAlertOpen(true)
  }
  const confirmDeleteConfig = async () => {
    if (configToDelete === undefined) {
      await sendNotification({
        title: "Error",
        body: "Configuration id is undefined.",
        icon: "error",
      })
      return
    }

    try {
      await invoke("delete_config", { id: configToDelete })
      const updatedConfigs: Status[] = await invoke("get_configs")
      setConfigs(updatedConfigs)

      await sendNotification({
        title: "Success",
        body: "Configuration deleted successfully.",
        icon: "success",
      })
    } catch (error) {
      console.error("Failed to delete configuration:", error)
      await sendNotification({
        title: "Error",
        body: `Failed to delete configuration:", "unknown error"`,
        icon: "error",
      })
    }

    // Close the alert dialog
    setIsAlertOpen(false)
  }

  const stopPortForwarding = async () => {
    setIsStopping(true)
    try {
      const responses: Response[] = await invoke("stop_port_forward")

      // Determine if all configs were successfully stopped
      const allStopped = responses.every((res) => res.status === 0)

      if (allStopped) {
        const updatedConfigs = configs.map((config) => ({
          ...config,
          isRunning: false, // Set isRunning to false for all configs
        }))

        setConfigs(updatedConfigs)
        setIsPortForwarding(false)
        await sendNotification({
          title: "Success",
          body: "Port forwarding stopped successfully for all configurations.",
          icon: "success",
        })
      } else {
        // Handle the case where some configs failed to stop
        const errorMessages = responses
          .filter((res) => res.status !== 0)
          .map((res) => `${res.service}: ${res.stderr}`)
          .join(", ")

        await sendNotification({
          title: "Error",
          body: `Port forwarding failed for some configurations: ${errorMessages}`,
          icon: "error",
        })
      }
    } catch (error) {
      console.error("An error occurred while stopping port forwarding:", error)
      await sendNotification({
        title: "Error",
        body: `An error occurred while stopping port forwarding: ${error}`,
        icon: "error",
      })
    }
    setIsStopping(false)
  }
  const quitApp = () => {
    invoke("quit_app")
  }

  const cardBg = useColorModeValue("gray.800", "gray.800")
  const textColor = useColorModeValue("gray.100", "gray.100")

  return (
    <Center h="90vh" margin="0" borderRadius="20px">
      {" "}
      {/* Setting the height to 100vh ensures it takes the full viewport height */}
      <VStack
        p={4}
        shadow="md"
        margin="0"
        position="absolute" // Changed from "absolute" to "relative" for alignment
        width="95%"
        height="95%"
        maxWidth="600px"
        maxHeight="500px" // Adjust this value to change the maximum height
        bg={cardBg} // Add a background to the card for better visibility
        borderRadius="20px" // Optional: add slight rounding of corners
        mb={10}
      >
        <Heading as="h1" size="lg" color="white" mb={1} marginTop={2}>
          <Image borderRadius="full" boxSize="100px" src={logo} />
        </Heading>
        <Center>
          <Modal isOpen={isModalOpen} onClose={closeModal} size="sm">
            <ModalContent mt="40px" width="fit-content">
              {" "}
              {/* Adjusts the top margin and centers horizontally */}
              <ModalCloseButton />
              <ModalBody mt="10px " width="fit-content" pb={2}>
                <FormControl>
                  <FormLabel>Context</FormLabel>
                  <Input
                    value={newConfig.context}
                    name="context"
                    onChange={handleInputChange}
                    size="sm"
                  />
                  <FormLabel>Namespace</FormLabel>
                  <Input
                    value={newConfig.namespace}
                    name="namespace"
                    onChange={handleInputChange}
                    size="sm"
                  />
                  <FormLabel>Service</FormLabel>
                  <Input
                    value={newConfig.service}
                    name="service"
                    onChange={handleInputChange}
                    size="sm"
                  />
                  <FormLabel>Local Port</FormLabel>
                  <Input
                    value={newConfig.local_port}
                    name="local_port"
                    onChange={handleInputChange}
                    size="sm"
                  />
                  <FormLabel>Remote Port</FormLabel>
                  <Input
                    value={newConfig.remote_port}
                    name="remote_port"
                    onChange={handleInputChange}
                    size="sm"
                  />
                </FormControl>
              </ModalBody>
              <ModalFooter>
                <Button
                  colorScheme="ghost"
                  variant="outline"
                  size="sm"
                  mr={6}
                  onClick={closeModal}
                >
                  Close
                </Button>
                {isEdit ? (
                  <Button
                    colorScheme="blue"
                    size="sm"
                    mr={1}
                    onClick={handleSaveConfig}
                  >
                    Save Changes
                  </Button>
                ) : (
                  <Button
                    colorScheme="facebook"
                    size="sm"
                    mr={1}
                    onClick={handleSaveConfig}
                  >
                    Add Config
                  </Button>
                )}
              </ModalFooter>
            </ModalContent>
          </Modal>
        </Center>
        <Stack direction="row" spacing={4} align="center" marginTop={2} mb={2}>
          <Button
            leftIcon={<MdRefresh />}
            colorScheme="facebook"
            isLoading={isInitiating}
            loadingText="Starting..."
            onClick={initiatePortForwarding}
            isDisabled={isPortForwarding}
          >
            Start Forward
          </Button>
          <Button
            leftIcon={<MdClose />}
            colorScheme="facebook"
            isLoading={isStopping}
            loadingText="Stopping..."
            onClick={stopPortForwarding}
            isDisabled={!isPortForwarding}
          >
            Stop Forward
          </Button>
        </Stack>
        {/* Your table UI */}
        <Table variant="simple" size="sm" align="center" marginTop={4}>
          <Thead>
            <Tr>
              <Th>Service</Th>
              <Th>context</Th>
              <Th>Namespace</Th>
              <Th>Local Port</Th>
              <Th>Status</Th>
              <Th>Action</Th>
            </Tr>
          </Thead>
          <Tbody>
            {configs.map((config) => (
              <Tr key={config.id}>
                <Td color={textColor}>{config.service}</Td>
                <Td color={textColor}>{config.context}</Td>
                <Td color={textColor}>{config.namespace}</Td>
                <Td color={textColor}>{config.local_port}</Td>

                <Td
                  color={config.isRunning ? "green.100" : "red.100"}
                  p={1}
                  textAlign="center"
                >
                  <StatusIcon isRunning={config.isRunning} />
                </Td>
                <Td>
                  <HStack spacing="0.1">
                    <IconButton
                      aria-label="Edit config"
                      icon={<MdEdit />}
                      size="sm"
                      colorScheme="blue"
                      onClick={() => handleEditConfig(config.id)}
                      variant="ghost"
                    />
                    <IconButton
                      aria-label="Delete config"
                      icon={<MdDelete />}
                      size="sm"
                      colorScheme="red"
                      onClick={() => handleDeleteConfig(config.id)}
                      variant="ghost"
                    />
                  </HStack>
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
                </Td>
              </Tr>
            ))}
          </Tbody>
        </Table>
        <Button
          leftIcon={<MdAdd />}
          onClick={openModal}
          colorScheme="facebook"
          size="xs"
          ml={450}
        >
          Add Config
        </Button>
      </VStack>
      {/* Quit IconButton */}
      <IconButton
        icon={<MdClose />}
        aria-label="Quit application"
        variant="solid"
        position="fixed"
        top={5}
        right={7}
        onClick={quitApp}
        isRound={false}
        size="xs"
        colorScheme="facebook"
      />
    </Center>
  )
}

export default App
