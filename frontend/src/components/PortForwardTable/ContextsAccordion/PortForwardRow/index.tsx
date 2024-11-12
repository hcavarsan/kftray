/* eslint-disable complexity */
import React, { useEffect, useRef, useState } from 'react'
import { ExternalLinkIcon } from 'lucide-react'

import {
  Box,
  Button,
  DialogActionTrigger,
  DialogBody,
  DialogCloseTrigger,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogRoot,
  DialogTitle,
  Flex,
  IconButton,
  Table,
  Text,
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
import { open as openShell } from '@tauri-apps/api/shell'
import { invoke } from '@tauri-apps/api/tauri'

import { Checkbox } from '@/components/ui/checkbox'
import { useColorModeValue } from '@/components/ui/color-mode'
import {
  MenuContent,
  MenuItem,
  MenuRoot,
  MenuTrigger,
} from '@/components/ui/menu'
import { Switch } from '@/components/ui/switch'
import { useCustomToast } from '@/components/ui/toaster'
import { Tooltip } from '@/components/ui/tooltip'
import { PortForwardRowProps } from '@/types'

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
  const { open, onOpen } = useDisclosure()
  const textColor = useColorModeValue('gray.300', 'gray.300')
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

    openShell(`http://${baseUrl}:${config.local_port}`).catch(error => {
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
    handleDeleteConfig(config.id)
    setIsAlertOpen(true)
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
      <Table.Row key={config.id}>
        {showContext && <Table.Cell>{config.context}</Table.Cell>}
        <Table.Cell
          color={textColor}
          fontFamily={fontFamily}
          fontSize={fontSize}
          width='39%'
        >
          <Checkbox
            size='xs'
            checked={selected || config.is_running}
            onChange={event => {
              event.stopPropagation()
              onSelectionChange(!selected)
            }}
            disabled={config.is_running}
            ml={1}
            mr={2}
            mt={2}
            variant='outline'
          />
          {config.alias}
          <Tooltip content={tooltipLabel}>
            <IconButton
              size='xs'
              aria-label='Info configuration'
              variant='ghost'
            >
              {infoIcon}
            </IconButton>
          </Tooltip>
        </Table.Cell>

        <Table.Cell
          color={textColor}
          fontFamily={fontFamily}
          fontSize={fontSize}
          textAlign='center'
        >
          <Text ml={-3}>{config.local_port}</Text>
        </Table.Cell>

        <Table.Cell>
          <Flex alignItems='center'>
            <Switch
              ml={2}
              colorPalette='facebook'
              checked={config.is_running && !isInitiating}
              size='sm'
              onCheckedChange={e => togglePortForwarding(e.checked)}
              disabled={isInitiating}
            />
            {config.is_running && (
              <Tooltip content='Open URL'>
                <IconButton
                  aria-label='Open local URL'
                  variant='ghost'
                  size='xs'
                  onClick={handleOpenLocalURL}
                  css={{
                    _hover: {
                      background: 'none',
                      transform: 'none',
                    },
                  }}
                >
                  {openLocalURLIcon}
                </IconButton>
              </Tooltip>
            )}
            {config.is_running &&
              (config.workload_type === 'service' ||
                config.workload_type === 'pod') &&
              config.protocol === 'tcp' &&
              httpLogsEnabled[config.id] && (
              <Tooltip content='HTTP trace logs'>
                <IconButton
                  aria-label='HTTP trace logs'
                  variant='ghost'
                  size='xs'
                  onClick={handleInspectLogs}
                  css={{
                    _hover: {
                      background: 'none',
                      transform: 'none',
                    },
                  }}
                >
                  <FontAwesomeIcon
                    icon={faFileAlt}
                    style={{ fontSize: '10px' }}
                  />
                </IconButton>
              </Tooltip>
            )}
          </Flex>
        </Table.Cell>

        <Table.Cell fontSize={fontSize} textAlign='center'>
          <MenuRoot>
            <MenuTrigger asChild>
              <IconButton aria-label='Options' variant='ghost' size='xs' ml={2}>
                <FontAwesomeIcon icon={faBars} style={{ fontSize: '10px' }} />
              </IconButton>
            </MenuTrigger>
            <MenuContent>
              <MenuItem
                value='edit'
                onClick={() => handleEditConfig(config.id)}
              >
                <FontAwesomeIcon
                  icon={faPen}
                  style={{ fontSize: '10px', marginRight: '8px' }}
                />
                <Box flex='1'>Edit</Box>
              </MenuItem>

              <MenuItem
                value='delete'
                onClick={handleDeleteClick}
              >
                <FontAwesomeIcon
                  icon={faTrash}
                  style={{ fontSize: '10px', marginRight: '8px' }}
                />
                <Box flex='1'>Delete</Box>
              </MenuItem>

              {config.protocol === 'tcp' && (
                <MenuItem value='http-logs' onClick={handleToggleHttpLogs}>
                  <FontAwesomeIcon
                    icon={faFileAlt}
                    style={{ fontSize: '10px', marginRight: '8px' }}
                  />
                  <Box flex='1'>
                    {httpLogsEnabled[config.id]
                      ? 'Disable HTTP Logs'
                      : 'Enable HTTP Logs'}
                  </Box>
                </MenuItem>
              )}
            </MenuContent>
          </MenuRoot>
        </Table.Cell>
      </Table.Row>

      {isAlertOpen && (
        <DialogRoot role='alertdialog'>
          <DialogContent>
            <DialogHeader>
              <DialogTitle>Delete Configuration</DialogTitle>
            </DialogHeader>
            <DialogBody>
              Are you sure? You can&apos;t undo this action afterwards.
            </DialogBody>
            <DialogFooter>
              <DialogActionTrigger asChild>
                <Button>Cancel</Button>
              </DialogActionTrigger>
              <Button colorPalette='red' onClick={confirmDeleteConfig} ml={3}>
                Delete
              </Button>
            </DialogFooter>
            <DialogCloseTrigger />
          </DialogContent>
        </DialogRoot>
      )}
    </>
  )
}

export default PortForwardRow
