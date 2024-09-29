/* eslint-disable complexity */
import React, { useEffect, useRef, useState } from 'react'

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
  Menu,
  MenuButton,
  MenuItem,
  MenuList,
  Portal,
  Switch,
  Td,
  Text,
  Tooltip,
  Tr,
  useColorModeValue,
  useDisclosure,
} from '@chakra-ui/react'
import {
  faBars,
  faFileAlt,
  faInfoCircle,
  faPen,
  faTrash,
} from '@fortawesome/free-solid-svg-icons'
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
  showContext = false,
  selected,
  onSelectionChange,
  isInitiating,
  setIsInitiating,
}) => {
  const { isOpen, onOpen } = useDisclosure()
  const textColor = useColorModeValue('gray.300', 'gray.300')
  const cancelRef = React.useRef<HTMLButtonElement>(null)
  const toast = useCustomToast()
  const [httpLogsEnabled, setHttpLogsEnabled] = useState<{
    [key: string]: boolean
  }>({})

  const prevConfigIdRef = useRef<number | null>(null)

  useEffect(() => {
    if (prevConfigIdRef.current !== config.id) {
      prevConfigIdRef.current = config.id

      const fetchHttpLogState = async () => {
        try {
          const enabled = await invoke('get_http_logs_cmd', {
            configId: config.id,
          })

          setHttpLogsEnabled(prevState => ({
            ...prevState,
            [config.id]: enabled,
          }))
        } catch (error) {
          console.error('Error fetching HTTP log state:', error)
        }
      }

      setHttpLogsEnabled(prevState => ({
        ...prevState,
        [config.id]: false,
      }))

      fetchHttpLogState()
    }
  }, [config.id])

  const handleOpenLocalURL = () => {
    const baseUrl = config.domain_enabled ? config.alias : config.local_address

    open(`http://${baseUrl}:${config.local_port}`).catch(error => {
      console.error('Error opening the URL:', error)
    })
  }

  const openLocalURLIcon = <ExternalLinkIcon style={{ fontSize: '10px' }} />

  const startPortForwarding = async () => {
    try {
      if (
        (config.workload_type === 'service' ||
          config.workload_type === 'pod') &&
        config.protocol === 'tcp'
      ) {
        await invoke('start_port_forward_tcp_cmd', { configs: [config] })
      } else if (
        config.workload_type.startsWith('proxy') ||
        ((config.workload_type === 'service' ||
          config.workload_type === 'pod') &&
          config.protocol === 'udp')
      ) {
        await invoke('deploy_and_forward_pod_cmd', { configs: [config] })
      } else {
        throw new Error(`Unsupported workload type: ${config.workload_type}`)
      }
    } catch (error) {
      console.error('An error occurred during port forwarding start:', error)
      const errorMessage =
        error instanceof Error ? error.message : String(error)

      toast({
        title: 'Error starting port forwarding',
        description: errorMessage,
        status: 'error',
      })
    }
  }

  const stopPortForwarding = async () => {
    try {
      if (
        (config.workload_type === 'service' ||
          config.workload_type === 'pod') &&
        config.protocol === 'tcp'
      ) {
        await invoke('stop_port_forward_cmd', {
          serviceName: config.service,
          configId: config.id.toString(),
        })
      } else if (
        config.workload_type.startsWith('proxy') ||
        ((config.workload_type === 'service' ||
          config.workload_type === 'pod') &&
          config.protocol === 'udp')
      ) {
        await invoke('stop_proxy_forward_cmd', {
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

  const handleInspectLogs = async () => {
    try {
      const logFileName = `${config.id}_${config.local_port}.log`

      await invoke('open_log_file', { logFileName: logFileName })
    } catch (error) {
      console.error('Error opening log file:', error)
      const errorMessage =
        error instanceof Error ? error.message : String(error)

      toast({
        title: 'Error opening log file',
        description: errorMessage,
        status: 'error',
        duration: 3000,
        isClosable: true,
      })
    }
  }

  const handleToggleHttpLogs = async () => {
    try {
      const newState = !httpLogsEnabled[config.id]

      await invoke('set_http_logs_cmd', {
        configId: config.id,
        enable: newState,
      })
      setHttpLogsEnabled(prevState => ({ ...prevState, [config.id]: newState }))
    } catch (error) {
      console.error('Error toggling HTTP logs:', error)
      const errorMessage =
        error instanceof Error ? error.message : String(error)

      toast({
        title: 'Error toggling HTTP logs',
        description: errorMessage,
        status: 'error',
      })
    }
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
        : config.workload_type === 'pod'
          ? 'Pod'
          : 'Service'}
      <br />
      <Box as='span' fontWeight='semibold'>
        {config.workload_type.startsWith('proxy')
          ? 'Remote Address:'
          : config.workload_type === 'pod'
            ? 'Pod Label:'
            : 'Service:'}
      </Box>{' '}
      {config.workload_type.startsWith('proxy')
        ? config.remote_address
        : config.workload_type === 'pod'
          ? config.target
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

  const fontFamily = '\'Open Sans\', sans-serif'
  const fontSize = '13px'

  return (
    <>
      <Tr key={config.id}>
        {showContext && <Td>{config.context}</Td>}
        <Td
          color={textColor}
          fontFamily={fontFamily}
          fontSize={fontSize}
          width='39%'
        >
          <Checkbox
            size='sm'
            isChecked={selected || config.is_running}
            onChange={event => {
              event.stopPropagation()
              onSelectionChange(!selected)
            }}
            disabled={config.is_running}
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

        <Td
          color={textColor}
          fontFamily={fontFamily}
          fontSize={fontSize}
          textAlign='center'
        >
          <Text ml={-3}>{config.local_port}</Text>
        </Td>

        <Td>
          <Flex alignItems='center'>
            <Switch
              ml={2}
              colorScheme='facebook'
              isChecked={config.is_running && !isInitiating}
              size='sm'
              onChange={e => togglePortForwarding(e.target.checked)}
              isDisabled={isInitiating}
            />
            {config.is_running && (
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
            {config.is_running &&
              (config.workload_type === 'service' ||
                config.workload_type === 'pod') &&
              config.protocol === 'tcp' &&
              httpLogsEnabled[config.id] && (
              <Tooltip
                hasArrow
                label='HTTP trace logs'
                placement='top-start'
                bg='gray.300'
                p={1}
                size='xs'
                fontSize='xs'
              >
                <IconButton
                  aria-label='HTTP trace logs'
                  icon={
                    <FontAwesomeIcon
                      icon={faFileAlt}
                      style={{ fontSize: '10px' }}
                    />
                  }
                  onClick={handleInspectLogs}
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
        <Td fontSize={fontSize} align='center'>
          <Menu>
            <MenuButton
              as={IconButton}
              aria-label='Options'
              icon={
                <FontAwesomeIcon icon={faBars} style={{ fontSize: '10px' }} />
              }
              variant='ghost'
              size='xs'
              ml={2}
            />
            <Portal>
              <MenuList zIndex='popover' fontSize='xs' minW='150px'>
                <MenuItem
                  icon={
                    <FontAwesomeIcon
                      icon={faPen}
                      style={{ fontSize: '10px' }}
                    />
                  }
                  onClick={() => handleEditConfig(config.id)}
                >
                  Edit
                </MenuItem>
                <MenuItem
                  icon={
                    <FontAwesomeIcon
                      icon={faTrash}
                      style={{ fontSize: '10px' }}
                    />
                  }
                  onClick={() => {
                    setIsAlertOpen(true)
                    handleDeleteClick()
                    handleDeleteConfig(config.id)
                  }}
                >
                  Delete
                </MenuItem>
                {config.protocol === 'tcp' && (
                  <MenuItem
                    icon={
                      <FontAwesomeIcon
                        icon={faFileAlt}
                        style={{ fontSize: '10px' }}
                      />
                    }
                    onClick={handleToggleHttpLogs}
                  >
                    {httpLogsEnabled[config.id]
                      ? 'Disable HTTP Logs'
                      : 'Enable HTTP Logs'}
                  </MenuItem>
                )}
              </MenuList>
            </Portal>
          </Menu>
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
