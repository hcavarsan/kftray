import React, { useEffect, useState } from 'react'
import { MdClose } from 'react-icons/md'

import {
  Box,
  Center,
  IconButton,
  useColorModeValue,
  VStack,
} from '@chakra-ui/react'
import { open, save } from '@tauri-apps/api/dialog'
import { readTextFile, writeTextFile } from '@tauri-apps/api/fs'
import { sendNotification } from '@tauri-apps/api/notification'
import { invoke } from '@tauri-apps/api/tauri'

import { AddConfigModal } from './add-config'
import { Footer } from './footer'
import { Header } from './header'
import { PortForwardTable } from './portforward-table'

interface Response {
  id: number
  service: string
  context: string
  local_port: number
  status: number
  namespace: string
  remote_port: number
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
  local_port: number
  isRunning: boolean
  namespace: string
  remote_port: number
  cancelRef: React.RefObject<HTMLButtonElement>
}

const initalRemotePort = 0
const initialLocalPort = 0
const initialId = 0
const initialStatus = 0

const KFTray = () => {
  const [isModalOpen, setIsModalOpen] = useState(false)
  const [isEdit, setIsEdit] = useState(false)
  const [newConfig, setNewConfig] = useState({
    id: initialId,
    service: '',
    context: '',
    local_port: initialLocalPort,
    remote_port: initalRemotePort,
    namespace: '',
  })
  const openModal = () => {
    setNewConfig({
      id: initialId,
      service: '',
      context: '',
      local_port: initialLocalPort,
      remote_port: initalRemotePort,
      namespace: '',
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
    const updatedValue =
      name === 'local_port' || name === 'remote_port' ? parseInt(value) : value


    setNewConfig(prev => ({ ...prev, [name]: updatedValue }))
  }
  const cancelRef = React.useRef<HTMLElement>(null)
  const [isInitiating, setIsInitiating] = useState(false)
  const [isStopping, setIsStopping] = useState(false)
  const [isPortForwarding, setIsPortForwarding] = useState(false)
  const [configs, setConfigs] = useState<Status[]>([])
  const [isAlertOpen, setIsAlertOpen] = useState(false)
  const [configToDelete, setConfigToDelete] = useState<number | undefined>(
    undefined,
  )

  useEffect(() => {
    const fetchConfigs = async () => {
      try {
        const configsResponse = await invoke<Status[]>('get_configs')

        setConfigs(
          configsResponse.map(config => ({
            ...config,
            // Since we don't know if they are running initially, set them all to false
            isRunning: false,
          })),
        )
      } catch (error) {
        console.error('Failed to fetch configs:', error)
        // Handle error appropriately
      }
    }

    fetchConfigs()
    // You might want to set the initial window size here as well
  }, [])

  const handleExportConfigs = async () => {
    try {
      // Inform backend that save dialog is about to open
      await invoke('open_save_dialog')

      const json = await invoke('export_configs')


      if (typeof json !== 'string') {
        throw new Error('The exported config is not a string')
      }

      const filePath = await save({
        defaultPath: 'configs.json',
        filters: [{ name: 'JSON', extensions: ['json'] }],
      })

      // Inform backend that save dialog has closed
      await invoke('close_save_dialog')

      if (filePath) {
        await writeTextFile(filePath, json)
        await sendNotification({
          title: 'Success',
          body: 'Configuration exported successfully.',
          icon: 'success',
        })
      }
    } catch (error) {
      console.error('Failed to export configs:', error)
      await sendNotification({
        title: 'Error',
        body: 'Failed to export configs.',
        icon: 'error',
      })
    }
  }
  const handleImportConfigs = async () => {
    try {
      await invoke('open_save_dialog')

      const selected = await open({
        filters: [
          {
            name: 'JSON',
            extensions: ['json'],
          },
        ],
        multiple: false,
      })


      await invoke('close_save_dialog')
      if (typeof selected === 'string') {
        // A file was selected, handle the file content
        const jsonContent = await readTextFile(selected)


        await invoke('import_configs', { json: jsonContent })

        // Fetch and update the list of configurations
        const updatedConfigs = await invoke<Status[]>('get_configs')


        setConfigs(updatedConfigs)

        // Show a success notification to the user
        await sendNotification({
          title: 'Success',
          body: 'Configurations imported successfully.',
          icon: 'success',
        })
      } else {
        // File dialog was cancelled
        console.log('No file was selected or the dialog was cancelled.')
      }
    } catch (error) {
      // Log any errors that arise
      console.error('Error during import:', error)
      await sendNotification({
        title: 'Error',
        body: 'Failed to import configurations.',
        icon: 'error',
      })
    }
  }
  const handleEditConfig = async (id: number) => {
    try {
      const configToEdit = await invoke<Config>('get_config', { id })


      setNewConfig({
        // populate the state with the fetched config
        id: configToEdit.id,
        service: configToEdit.service,
        namespace: configToEdit.namespace,
        local_port: configToEdit.local_port,
        remote_port: configToEdit.remote_port,
        context: configToEdit.context,
      })
      setIsEdit(true) // Set isEdit to true because we are editing
      setIsModalOpen(true)
    } catch (error) {
      console.error(
        `Failed to fetch the config for editing with id ${id}:`,
        error,
      )
      // Handle the error...
    }
  }
  const handleEditSubmit = async (e: React.FormEvent) => {
    e.preventDefault() // Prevent the default form submit action

    // Check if the port numbers are within the correct range

    try {
      // Construct the edited config object
      const editedConfig = {
        id: newConfig.id, // Include the id for updating the existing record
        service: newConfig.service,
        context: newConfig.context,
        local_port: newConfig.local_port,
        remote_port: newConfig.remote_port,
        namespace: newConfig.namespace,
      }

      await invoke('update_config', { config: editedConfig })

      // Fetch the updated configurations
      const updatedConfigs = await invoke<Status[]>('get_configs')


      setConfigs(updatedConfigs)

      // Show success notification
      await sendNotification({
        title: 'Success',
        body: 'Configuration updated successfully.',
        icon: 'success',
      })

      closeModal() // Close the modal after successful update
    } catch (error) {
      console.error('Failed to update config:', error)
      // Handle errors
      await sendNotification({
        title: 'Error',
        body: 'Failed to update configuration.',
        icon: 'error',
      })
    }
  }
  const handleSaveConfig = async (e: React.FormEvent) => {
    e.preventDefault() // Prevent the default form submit action

    const configToSave = {
      id: isEdit ? newConfig.id : undefined,
      service: newConfig.service,
      context: newConfig.context,
      local_port: newConfig.local_port,
      remote_port: newConfig.remote_port,
      namespace: newConfig.namespace,
    }

    try {
      // Check if we're adding a new config or updating an existing one
      if (isEdit) {
        // Update existing config
        await invoke('update_config', { config: configToSave })
      } else {
        // Insert new config
        await invoke('insert_config', { config: configToSave })
      }

      // Fetch and update the list of configurations
      const updatedConfigs = await invoke<Status[]>('get_configs')


      setConfigs(updatedConfigs)

      // Show a success notification to the user
      await sendNotification({
        title: 'Success',
        body: `Configuration ${isEdit ? 'updated' : 'added'} successfully.`,
        icon: 'success',
      })

      // Close the modal after successful insert/update
      closeModal()
    } catch (error) {
      console.error(`Failed to ${isEdit ? 'update' : 'insert'} config:`, error)

      // Handle errors, such as showing an error notification
      await sendNotification({
        title: 'Error',
        body: `Failed to ${
          isEdit ? 'update' : 'add'
        } configuration. Error: ${error}`,
        icon: 'error',
      })
    }
  }

  const initiatePortForwarding = async () => {
    setIsInitiating(true)
    try {
      const configsToSend = configs.map(config => ({
        // Remove the id property if it's not expected by your command
        // Transform local_port and remote_port to the correct type if needed
        ...config,
        local_port: config.local_port,
        remote_port: config.remote_port,
      }))

      const responses = await invoke<Response[]>('start_port_forward', {
        configs: configsToSend,
      })

      // Update each config with its new running status, depending on the response status.
      const updatedConfigs = configs.map(config => {
        const relatedResponse = responses.find(res => res.id === config.id)



        return {
          ...config,
          isRunning: relatedResponse ? relatedResponse.status === initialStatus : false,
        }
      })

      setConfigs(updatedConfigs)
      setIsPortForwarding(true)
    } catch (error) {
      console.error(
        'An error occurred while initiating port forwarding:',
        error,
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
        title: 'Error',
        body: 'Configuration id is undefined.',
        icon: 'error',
      })

      return
    }

    try {
      await invoke('delete_config', { id: configToDelete })
      const updatedConfigs = await invoke<Status[]>('get_configs')


      setConfigs(updatedConfigs)

      await sendNotification({
        title: 'Success',
        body: 'Configuration deleted successfully.',
        icon: 'success',
      })
    } catch (error) {
      console.error('Failed to delete configuration:', error)
      await sendNotification({
        title: 'Error',
        body: 'Failed to delete configuration:", "unknown error"',
        icon: 'error',
      })
    }

    // Close the alert dialog
    setIsAlertOpen(false)
  }

  const stopPortForwarding = async () => {
    setIsStopping(true)
    try {
      const responses = await invoke<Response[]>('stop_port_forward')

      // Determine if all configs were successfully stopped
      const allStopped = responses.every(res => res.status === initialStatus)

      if (allStopped) {
        const updatedConfigs = configs.map(config => ({
          ...config,
          isRunning: false, // Set isRunning to false for all configs
        }))

        setConfigs(updatedConfigs)
        setIsPortForwarding(false)
        await sendNotification({
          title: 'Success',
          body: 'Port forwarding stopped successfully for all configurations.',
          icon: 'success',
        })
      } else {
        // Handle the case where some configs failed to stop
        const errorMessages = responses
        .filter(res => res.status !== initialStatus)
        .map(res => `${res.service}: ${res.stderr}`)
        .join(', ')

        await sendNotification({
          title: 'Error',
          body: `Port forwarding failed for some configurations: ${errorMessages}`,
          icon: 'error',
        })
      }
    } catch (error) {
      console.error('An error occurred while stopping port forwarding:', error)
      await sendNotification({
        title: 'Error',
        body: `An error occurred while stopping port forwarding: ${error}`,
        icon: 'error',
      })
    }
    setIsStopping(false)
  }
  const quitApp = () => {
    invoke('quit_app')
  }

  const cardBg = useColorModeValue('gray.800', 'gray.800')

  return (
    <Center h='100%' w='100%' overflow='hidden' margin='0'>
      {/* Wrapper to maintain borderRadius, with overflow hidden */}
      <Box
        width='100%'
        height='75vh'
        maxH='95vh'
        maxW='600px'
        overflow='hidden'
        borderRadius='20px'
        bg={cardBg}
        boxShadow={`
      /* Inset shadow for top & bottom inner border effect using dark gray */
      inset 0 2px 4px rgba(0, 0, 0, 0.3),
      inset 0 -2px 4px rgba(0, 0, 0, 0.3),
      /* Inset shadow for an inner border all around using dark gray */
      inset 0 0 0 4px rgba(45, 57, 81, 0.9)
    `}
      >
        {/* Scrollable VStack inside the wrapper */}
        <VStack
          css={{
            '&::-webkit-scrollbar': {
              width: '5px',
              background: 'transparent',
            },
            '&::-webkit-scrollbar-thumb': {
              background: '#555',
            },
            '&::-webkit-scrollbar-thumb:hover': {
              background: '#666',
            },
          }}
          h='100%'
          w='100%'
          maxW='100%'
          overflowY='auto'
          padding='20px' // Adjust padding to prevent content from touching the edges
          mt='5px'
        >
          <Header />

          <AddConfigModal
            isModalOpen={isModalOpen}
            closeModal={closeModal}
            newConfig={newConfig}
            handleInputChange={handleInputChange}
            handleSaveConfig={handleSaveConfig}
            isEdit={isEdit}
            handleEditSubmit={handleEditSubmit}
            cancelRef={cancelRef}
          />

          <PortForwardTable
            configs={configs}
            initiatePortForwarding={initiatePortForwarding}
            isInitiating={isInitiating}
            isStopping={isStopping}
            isPortForwarding={isPortForwarding}
            handleEditConfig={handleEditConfig}
            stopPortForwarding={stopPortForwarding}
            handleDeleteConfig={handleDeleteConfig}
            confirmDeleteConfig={confirmDeleteConfig}
            isAlertOpen={isAlertOpen}
            setIsAlertOpen={setIsAlertOpen}
          />

          <Footer
            openModal={openModal}
            handleExportConfigs={handleExportConfigs}
            handleImportConfigs={handleImportConfigs}
          />
        </VStack>
        <IconButton
          icon={<MdClose />}
          aria-label='Quit application'
          variant='solid'
          position='fixed'
          top={7}
          right={4}
          onClick={quitApp}
          isRound={false}
          size='xs'
          colorScheme='facebook'
        />
      </Box>
    </Center>
  )
}

export default KFTray
