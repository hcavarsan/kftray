import React, { useCallback, useEffect, useRef, useState } from 'react'
import { ExternalLinkIcon, FileIcon } from 'lucide-react'

import {
  Box,
  Button,
  DialogBackdrop,
  DialogCloseTrigger,
  DialogContent,
  DialogRoot,
  Flex,
  IconButton,
  Table,
  Text,
} from '@chakra-ui/react'
import {
  faBars,
  faInfoCircle,
  faPen,
  faTrash,
} from '@fortawesome/free-solid-svg-icons'
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome'
import { open as openShell } from '@tauri-apps/api/shell'
import { invoke } from '@tauri-apps/api/tauri'

import { Checkbox } from '@/components/ui/checkbox'
import {
  MenuContent,
  MenuItem,
  MenuRoot,
  MenuTrigger,
} from '@/components/ui/menu'
import { Switch } from '@/components/ui/switch'
import { toaster } from '@/components/ui/toaster'
import { Tooltip } from '@/components/ui/tooltip'
import { PortForwardRowProps } from '@/types'

import '../../styles.css'

const PortForwardRow: React.FC<PortForwardRowProps> = ({
  config,
  confirmDeleteConfig,
  handleDeleteConfig,
  handleEditConfig,
  selected,
  onSelectionChange,
  isInitiating,
  setIsInitiating,
}) => {
  const [httpLogsEnabled, setHttpLogsEnabled] = useState<{
    [key: string]: boolean
  }>({})
  const prevConfigIdRef = useRef<number | null>(null)
  const [isDeleteDialogOpen, setIsDeleteDialogOpen] = useState(false)

  const fetchHttpLogState = useCallback(async () => {
    try {
      const enabled = await invoke('get_http_logs_cmd', { configId: config.id })

      setHttpLogsEnabled(prev => ({ ...prev, [config.id]: enabled }))
    } catch (error) {
      console.error('Error fetching HTTP log state:', error)
    }
  }, [config.id])

  useEffect(() => {
    if (prevConfigIdRef.current !== config.id) {
      prevConfigIdRef.current = config.id
      fetchHttpLogState()
    }
  }, [config.id, fetchHttpLogState])

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
      toaster.error({
        title: 'Error toggling HTTP logs',
        description: error instanceof Error ? error.message : String(error),
        duration: 1000,
      })
    }
  }

  const handleInspectLogs = async () => {
    try {
      const logFileName = `${config.id}_${config.local_port}.log`

      await invoke('open_log_file', { logFileName: logFileName })
    } catch (error) {
      console.error('Error opening log file:', error)
      toaster.error({
        title: 'Error opening log file',
        description: error instanceof Error ? error.message : String(error),
        duration: 1000,
      })
    }
  }

  const handleOpenLocalURL = () => {
    const baseUrl = config.domain_enabled ? config.alias : config.local_address

    openShell(`http://${baseUrl}:${config.local_port}`).catch(console.error)
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
      setIsInitiating(false)
    }
  }

  const startPortForwarding = async () => {
    try {
      if (config.workload_type === 'proxy' || config.workload_type === 'expose') {
        await invoke('deploy_and_forward_pod_cmd', { configs: [config] })
      } else if (config.protocol === 'tcp') {
        await invoke('start_port_forward_tcp_cmd', { configs: [config] })
      } else if (config.protocol === 'udp') {
        await invoke('start_port_forward_udp_cmd', { configs: [config] })
      } else {
        throw new Error(`Unsupported configuration: workload_type=${config.workload_type}, protocol=${config.protocol}`)
      }
    } catch (error) {
      toaster.error({
        title: 'Error starting port forwarding',
        description: error instanceof Error ? error.message : String(error),
        duration: 1000,
      })
    }
  }

  const stopPortForwarding = async () => {
    try {
      await invoke('stop_port_forward_cmd', {
        configId: config.id.toString(),
      })
    } catch (error) {
      toaster.error({
        title: 'Error stopping port forwarding',
        description: error instanceof Error ? error.message : String(error),
        duration: 1000,
      })
    }
  }

  const handleOpenChange = (details: { open: boolean }) => {
    setIsDeleteDialogOpen(details.open)
  }

  const handleOpenDeleteDialog = () => {
    handleDeleteConfig(config.id)
    setIsDeleteDialogOpen(true)
  }

  return (
    <>
      <Table.Row className='table-row'>
        <Table.Cell className='table-cell'>
          <Flex align='center' gap={1.5}>
            <Tooltip content={getTooltipContent(config)} portalled>
              <Flex align='center' gap={1.5}>
                <Checkbox
                  size='xs'
                  checked={selected || config.is_running}
                  onCheckedChange={e => {
                    if (!config.is_running) {
                      onSelectionChange(e.checked === true)
                    }
                  }}
                  disabled={config.is_running}
                  className='checkbox'
                />

                <IconButton
                  size='xs'
                  variant='ghost'
                  aria-label='Info'
                  className='icon-button'
                >
                  <FontAwesomeIcon icon={faInfoCircle} size='xs' />
                </IconButton>

                <Text className='text-normal' truncate maxWidth='100px'>
                  {config.alias}
                </Text>
              </Flex>
            </Tooltip>
          </Flex>
        </Table.Cell>

        <Table.Cell className='table-cell'>
          <Text>{config.local_port}</Text>
        </Table.Cell>

        <Table.Cell className='table-cell'>
          <Flex align='center' gap={1.5}>
            <Switch
              size='sm'
              checked={config.is_running && !isInitiating}
              onCheckedChange={details => togglePortForwarding(details.checked)}
              disabled={isInitiating}
              data-loading={isInitiating ? '' : undefined}
              unstyled={true}
              className='switch'
            />
            {config.is_running && (
              <Flex gap={1.5}>
                <Tooltip content='Open in browser' portalled>
                  <IconButton
                    size='2xs'
                    variant='ghost'
                    aria-label='Open URL'
                    onClick={handleOpenLocalURL}
                    className='icon-button'
                  >
                    <ExternalLinkIcon size={12} />
                  </IconButton>
                </Tooltip>
                {httpLogsEnabled[config.id] && (
                  <Tooltip content='View HTTP logs' portalled>
                    <IconButton
                      size='2xs'
                      variant='ghost'
                      onClick={handleInspectLogs}
                      aria-label='HTTP Logs'
                      className='icon-button'
                    >
                      <FileIcon size={10} />
                    </IconButton>
                  </Tooltip>
                )}
              </Flex>
            )}
          </Flex>
        </Table.Cell>

        <Table.Cell className='table-cell'>
          <MenuRoot>
            <MenuTrigger asChild>
              <IconButton
                size='xs'
                ml={5}
                variant='ghost'
                aria-label='Actions'
                className='icon-button'
              >
                <FontAwesomeIcon icon={faBars} size='xs' />
              </IconButton>
            </MenuTrigger>
            <MenuContent className='menu-content'>
              <MenuItem
                className='menu-item'
                value='edit'
                onClick={() => handleEditConfig(config.id)}
              >
                <FontAwesomeIcon icon={faPen} size='xs' />
                <Text ml={2} fontSize='xs'>
                  Edit
                </Text>
              </MenuItem>
              <MenuItem
                className='menu-item'
                value='delete'
                onClick={handleOpenDeleteDialog}
              >
                <FontAwesomeIcon icon={faTrash} size='xs' />
                <Text ml={2} fontSize='xs'>
                  Delete
                </Text>
              </MenuItem>
              {config.protocol === 'tcp' && (
                <MenuItem
                  className='menu-item'
                  value='http-logs'
                  onClick={handleToggleHttpLogs}
                >
                  <FileIcon size={12} />
                  <Text ml={2} fontSize='xs'>
                    {httpLogsEnabled[config.id] ? 'Disable' : 'Enable'} HTTP
                    Logs
                  </Text>
                </MenuItem>
              )}
            </MenuContent>
          </MenuRoot>
        </Table.Cell>
      </Table.Row>

      {isDeleteDialogOpen && (
        <DialogRoot open={isDeleteDialogOpen} onOpenChange={handleOpenChange}>
          <DialogBackdrop className='dialog-backdrop' />
          <DialogContent className='dialog-content'>
            <Box className='dialog-header'>
              <Text fontSize='sm' fontWeight='medium' color='gray.100'>
                Delete Configuration
              </Text>
            </Box>

            <Box className='dialog-body'>
              <Text fontSize='xs' color='gray.400'>
                Are you sure? You can&lsquo;t undo this action afterwards.
              </Text>
            </Box>

            <Box className='dialog-footer'>
              <DialogCloseTrigger asChild>
                <Button size='xs' variant='ghost' className='dialog-button'>
                  Cancel
                </Button>
              </DialogCloseTrigger>
              <Button
                size='xs'
                className='dialog-button dialog-button-primary'
                onClick={() => {
                  confirmDeleteConfig()
                  setIsDeleteDialogOpen(false)
                }}
              >
                Delete
              </Button>
            </Box>
          </DialogContent>
        </DialogRoot>
      )}
    </>
  )
}

const getTooltipContent = (config: any) => (
  <Box p={1.5}>
    <Text fontSize='xs'>
      <strong>Alias:</strong> {config.alias}
    </Text>
    <Text fontSize='xs'>
      <strong>Workload Type:</strong> {config.workload_type}
    </Text>
    <Text fontSize='xs'>
      <strong>Service:</strong> {config.service}
    </Text>
    <Text fontSize='xs'>
      <strong>Context:</strong> {config.context}
    </Text>
    <Text fontSize='xs'>
      <strong>Namespace:</strong> {config.namespace}
    </Text>
    <Text fontSize='xs'>
      <strong>Target Port:</strong> {config.remote_port}
    </Text>
    <Text fontSize='xs'>
      <strong>Local Address:</strong> {config.local_address}
    </Text>
    <Text fontSize='xs'>
      <strong>Protocol:</strong> {config.protocol}
    </Text>
    <Text fontSize='xs'>
      <strong>Domain Enabled:</strong> {config.domain_enabled ? 'Yes' : 'No'}
    </Text>
  </Box>
)

export default PortForwardRow
