// App.tsx
import React, { useEffect, useState } from "react"
import { ArrowRightIcon } from "@chakra-ui/icons"
import {
	Box,
	Button,
	Center,
	Flex,
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
	ModalHeader,
	ModalOverlay,
	Stack,
	Table,
	Tbody,
	Td,
	Text,
	Th,
	Thead,
	Tr,
	useColorMode,
	useColorModeValue,
	VStack,
} from "@chakra-ui/react"
import { sendNotification } from "@tauri-apps/api/notification"
import { invoke } from "@tauri-apps/api/tauri"
import { appWindow, LogicalSize } from "@tauri-apps/api/window"
import { MdAdd, MdClose, MdDelete, MdRefresh } from "react-icons/md"

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
	const [newConfig, setNewConfig] = useState({
		id: 0,
		service: "",
		context: "",
		local_port: "",
		remote_port: "",
		namespace: "",
	})
	const openModal = () => setIsModalOpen(true)
	const closeModal = () => setIsModalOpen(false)
	const handleInputChange = (e: React.ChangeEvent<HTMLInputElement>) => {
		const { name, value } = e.target
		setNewConfig((prev) => ({ ...prev, [name]: value }))
	}

	const { colorMode } = useColorMode()
	const [isInitiating, setIsInitiating] = useState(false)
	const [isStopping, setIsStopping] = useState(false)
	const [isPortForwarding, setIsPortForwarding] = useState(false)
	const [statuses, setStatuses] = useState<Status[]>([])
	const [configs, setConfigs] = useState<Status[]>([]) // Add a useState to hold your configs

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

	const handleSubmit = async (e: React.FormEvent) => {
		e.preventDefault() // Prevent the default form submit action

		try {
			const configToInsert = {
				service: newConfig.service,
				context: newConfig.context,
				local_port: parseInt(newConfig.local_port, 10), // Ensure you parse the string to a number
				remote_port: parseInt(newConfig.remote_port, 10), // Same here
				namespace: newConfig.namespace,
			}

			await invoke("insert_config", { config: configToInsert })

			// Assuming the `get_configs` function can return the updated list
			// including the newly inserted configuration, we then refetch the configurations.
			const updatedConfigs: Status[] = await invoke("get_configs")
			setConfigs(updatedConfigs)

			// Show a success notification to the user
			sendNotification({
				title: "Success",
				body: "Configuration added successfully.",
				icon: "success",
			})

			closeModal() // Close the modal after successful insert
		} catch (error) {
			console.error("Failed to insert config:", error)
			// Handle errors such as showing an error notification
			sendNotification({
				title: "Error",
				body: `Failed to add configuration:", "unknown error"`,
				icon: "error",
			})
		}
	}

	const initiatePortForwarding = async () => {
		setIsInitiating(true)
		try {
			const responses: Response[] = await invoke("port_forward")
			let allSuccess = true
			let errorMessages: string[] = []
			let successfulCommands: string[] = []

			const newStatuses = responses.map((res) => {
				// Log the response data:
				console.log(
					`Stdout: ${res.stdout}, Stderr: ${res.stderr}, Status: ${res.status}`
				)
				let isRunning = res.status === 0
				if (!isRunning) {
					allSuccess = false
					errorMessages.push(res.stderr ?? "An unknown error occurred.")
				} else {
					successfulCommands.push(res.service)
				}
				return {
					id: res.id,
					service: res.service,
					context: res.context,
					local_port: res.local_port,
					remote_port: res.remote_port,
					namespace: res.namespace,
					isRunning,
				}
			})

			setStatuses(newStatuses)
			setIsPortForwarding(allSuccess)
			setConfigs(newStatuses)
			if (!allSuccess) {
				for (let command of successfulCommands) {
					await invoke("stop_port_forward", { command })
				}
				await sendNotification({
					title: "Error",
					body: `Port forwarding failed for some configurations. Errors: ${errorMessages.join(
						", "
					)}`,
					icon: "error",
				})
			} else {
				await sendNotification({
					title: "Success",
					body: "Port forwarding initiated successfully for all configurations.",
					icon: "success",
				})
			}

			setIsPortForwarding(allSuccess)
		} catch (error) {
			let errorMessages = error ? [error] : ["An unknown error occurred."]
			await sendNotification({
				title: "Error",
				body: `Port forwarding failed for some configurations. Errors: ${errorMessages.join(
					", "
				)}`,
				icon: "error",
			})
		} finally {
			setIsInitiating(false) // Ensure the loading state is reset
		}
	}
	const handleDeleteConfig = async (id: number) => {
		console.log(`Attempting to invoke delete_config with id: ${id}`) // Check if `id` is undefined
		if (id === undefined) {
			sendNotification({
				title: "Error",
				body: "Configuration id is undefined.",
				icon: "error",
			})
			return
		}

		try {
			await invoke("delete_config", { id })
			const updatedConfigs: Status[] = await invoke("get_configs")
			setConfigs(updatedConfigs)

			sendNotification({
				title: "Success",
				body: "Configuration deleted successfully.",
				icon: "success",
			})
		} catch (error) {
			console.error("Failed to delete configuration:", error)
			sendNotification({
				title: "Error",
				body: `Failed to delete configuration:", "unknown error"`,
				icon: "error",
			})
		}
	}
	const stopPortForwarding = async () => {
		setIsStopping(true)
		try {
			const responses: Response[] = await invoke("stop_port_forward")
			let allStopped = true
			let errorMessages: string[] = []

			const newStatuses = responses.map((res) => {
				// Log the response data:
				console.log(
					`Stdout: ${res.stdout}, Stderr: ${res.stderr}, Status: ${res.status}`
				)
				let isRunning = res.status !== 0
				if (isRunning) {
					allStopped = false
					errorMessages.push(res.stderr ?? "An unknown error occurred.")
				}
				return {
					id: res.id,
					service: res.service,
					context: res.context,
					local_port: res.local_port,
					remote_port: res.remote_port,
					namespace: res.namespace,
					isRunning,
				}
			})

			setIsPortForwarding(!allStopped)
			setStatuses(newStatuses)
			setConfigs(newStatuses)
			if (!allStopped) {
				await sendNotification({
					title: "Error",
					body: `Port forwarding failed for some configurations. Errors: ${errorMessages.join(
						", "
					)}`,
					icon: "error",
				})
			} else {
				await sendNotification({
					title: "Success",
					body: "Port forwarding stopped successfully for all configurations.",
					icon: "success",
				})
			}
		} catch (error) {
			let errorMessages = error ? [error] : ["An unknown error occurred."]
			await sendNotification({
				title: "Error",
				body: `Port forwarding failed for some configurations. Errors: ${errorMessages.join(
					", "
				)}`,
				icon: "error",
			})
		} finally {
			setIsStopping(false) // Ensure the loading state is reset
		}
	}
	const quitApp = () => {
		invoke("quit_app")
	}

	const cardBg = useColorModeValue("gray.800", "gray.800")
	const textColor = useColorModeValue("gray.100", "gray.100")

	return (
		<Center h="100vh" bg="dark.700" margin="0">
			{" "}
			{/* Setting the height to 100vh ensures it takes the full viewport height */}
			<VStack
				spacing={2}
				position="relative" // Changed from "absolute" to "relative" for alignment
				width="95%"
				height="95%"
				maxWidth="700px"
				maxHeight="500px" // Adjust this value to change the maximum height
				p={5} // Add some padding
				bg={cardBg} // Add a background to the card for better visibility
				borderRadius="md" // Optional: add slight rounding of corners
				boxShadow="md" // Optional: some shadow for depth
			>
				<Heading as="h1" size="lg" color="white" mb={2} marginTop={2}>
					<Image borderRadius="full" boxSize="100px" src={logo} />
				</Heading>
				<Center>
					<Modal
						isOpen={isModalOpen}
						onClose={closeModal}
						size="sm"
					>
						<ModalOverlay />
						<ModalContent
							mt="10px"
							maxWidth="500px"
							width="fit-content"
							maxHeight="420px"
						>
							{" "}
							{/* Adjusts the top margin and centers horizontally */}
							<ModalCloseButton />
							<ModalBody
								mt="10px "
								width="fit-content"
								pb={2}
							>
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
									colorScheme="blue"
									mt="10px"
									size="sm"
									mr={6}
									onClick={closeModal}
								>
									Close
								</Button>
								<Button
									variant="ghost"
									mt="10px"
									size="sm"
									mr={6}
									onClick={handleSubmit}
								>
									Add Config
								</Button>
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
							<Th >Service</Th>
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
									<IconButton
										aria-label="Delete config"
										icon={<MdDelete />}
										size="sm"
										colorScheme="red"
										onClick={() => handleDeleteConfig(config.id)}
										variant="ghost"
									/>
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
				top={7}
				right={6}
				onClick={quitApp}
				isRound={false}
				size="xs"
				colorScheme="facebook"
			/>
		</Center>
	)
}

export default App
