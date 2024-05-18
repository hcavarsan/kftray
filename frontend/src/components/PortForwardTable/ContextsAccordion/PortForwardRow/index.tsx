import React from 'react'

import { ExternalLinkIcon } from '@chakra-ui/icons'
import {
  AlertDialog,
  AlertDialogBody,
  AlertDialogContent,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogOverlay,
  Box,
  Button,
  Checkbox,
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
import { open } from '@tauri-apps/api/shell'
import { invoke } from '@tauri-apps/api/tauri'

import { PortForwardRowProps } from '../../../../types'
import useCustomToast from '../../../CustomToast'

const PortForwardRow: React.FC<PortForwardRowProps> = ({
  config,
  confirmDeleteConfig,
  handleDeleteConfig,
  handleEditConfig,
  setIsAlertOpen,
  isAlertOpen,
  updateConfigRunningState,
  showContext = false,
  selected,
  onSelectionChange,
  updateSelectionState,
  isInitiating,
  setIsInitiating,
}) => {
  const { isOpen, onOpen } = useDisclosure()
  const textColor = useColorModeValue('gray.100', 'gray.100')
  const cancelRef = React.useRef<HTMLButtonElement>(null)
  const toast = useCustomToast()
  const handleOpenLocalURL = () => {
    const baseUrl = config.domain_enabled ? config.alias : config.local_address

    open(`http://${baseUrl}:${config.local_port}`).catch(error => {
      console.error('Error opening the URL:', error)
    })
  }

  const openLocalURLIcon = <ExternalLinkIcon style={{ fontSize: '10px' }} />

  const startPortForwarding = async () => {
    try {
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
      updateSelectionState(config.id, true)
    } catch (error) {
      console.error('An error occurred during port forwarding start:', error)
      const errorMessage =
        error instanceof Error ? error.message : String(error)

      toast({
        title: 'Error starting port forwarding',
        description: errorMessage,
        status: 'error',
      })

      updateConfigRunningState(config.id, false)
    }
  }

  const stopPortForwarding = async () => {
    try {
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
    } catch (error) {
      console.error('An error occurred during port forwarding stop:', error)
      const errorMessage =
        error instanceof Error ? error.message : String(error)

      toast({
        title: 'Error stopping port forwarding',
        description: errorMessage,
        status: 'error',
      })
    } finally {
      console.log('stopPortForwarding finally')
    }
  }

  const togglePortForwarding = async (isChecked: boolean) => {
    setIsInitiating(true)
    try {
      if (isChecked) {
        await startPortForwarding()
      } else {
        await stopPortForwarding()
      }
    } catch (error) {
      console.error('Error toggling port-forwarding:', error)
    } finally {
      console.log('togglePortForwarding finally')
      setIsInitiating(false)
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
        Local Address:
      </Box>{' '}
      {config.local_address}
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
      <Box as='span' fontWeight='semibold'>
        Domain Enabled:
      </Box>{' '}
      {config.domain_enabled ? 'true' : 'false'}
      <br />
      <Box as='span' fontWeight='semibold'>
        kubeconfig
      </Box>{' '}
      {config.kubeconfig}
      <br />
    </>
  )

  const fontFamily = '"Inter", sans-serif'

  return (
    <>
      <Tr key={config.id}>
        {showContext && <Td>{config.context}</Td>}
        <Td color={textColor} fontFamily={fontFamily} width='40%'>
          <Checkbox
            size='sm'
            isChecked={selected || config.isRunning}
            onChange={event => {
              event.stopPropagation()
              onSelectionChange(!selected)
            }}
            disabled={config.isRunning}
            ml={-4}
            mr={2}
            mt={1}
            variant='ghost'
          />
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
              colorScheme='facebook'
              isChecked={config.isRunning && !isInitiating}
              size='sm'
              onChange={e => togglePortForwarding(e.target.checked)}
              isDisabled={isInitiating}
            />
            {config.isRunning && (
              <Tooltip
                hasArrow
                label='Open URL'
                placement='top-start'
                bg='gray.300'
                p={1}
                size='xs'
                fontSize='xs'
              >
                <IconButton
                  aria-label='Open local URL'
                  icon={openLocalURLIcon}
                  onClick={handleOpenLocalURL}
                  size='xs'
                  variant='ghost'
                  _hover={{
                    background: 'none',
                    transform: 'none',
                  }}
                />
              </Tooltip>
            )}
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
          onClose={() => setIsAlertOpen(false)}
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
                <Button onClick={() => setIsAlertOpen(false)}>Cancel</Button>
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
