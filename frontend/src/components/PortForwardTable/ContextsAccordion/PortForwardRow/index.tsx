import React, { useCallback, useEffect, useRef, useState } from 'react'
import {
  ClipboardIcon,
  ExternalLinkIcon,
  FileIcon,
  SettingsIcon,
} from 'lucide-react'

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
import { listen } from '@tauri-apps/api/event'
import { open as openShell } from '@tauri-apps/api/shell'
import { invoke } from '@tauri-apps/api/tauri'

import HttpLogsConfigModal from '@/components/HttpLogsConfigModal'
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
  _isInitiating,
  setIsInitiating,
}) => {
  const [httpLogsEnabled, setHttpLogsEnabled] = useState<{
    [key: string]: boolean
  }>({})
  const prevConfigIdRef = useRef<number | null>(null)
  const [isDeleteDialogOpen, setIsDeleteDialogOpen] = useState(false)
  const [isHttpLogsConfigOpen, setIsHttpLogsConfigOpen] = useState(false)
  const [activePod, setActivePod] = useState<string | null>(null)
  const [localInitiating, setLocalInitiating] = useState(false)

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

  useEffect(() => {
    if (!config.is_running && !localInitiating) {
      setActivePod(null)
    } else if (config.is_running) {
      const fetchInitialPod = async () => {
        try {
          const podName = await invoke<string | null>('get_active_pod_cmd', {
            configId: config.id.toString(),
          })

          setActivePod(podName)
        } catch (error) {
          console.error('Error fetching initial active pod:', error)
          setActivePod(null)
        }
      }

      fetchInitialPod()
    }
  }, [config.is_running, config.id, localInitiating])

  useEffect(() => {
    const setupListener = async () => {
      const unlisten = await listen('active_pod_changed', (event: any) => {
        const { configId, podName } = event.payload

        if (configId === config.id.toString()) {
          setActivePod(podName)
        }
      })

      return unlisten
    }

    const unlistenPromise = setupListener()

    return () => {
      unlistenPromise.then(unlisten => unlisten())
    }
  }, [config.id])

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
      const logFileName = `${config.id}_${config.local_port}.http`

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

  const handleOpenHttpLogsConfig = () => {
    setIsHttpLogsConfigOpen(true)
  }

  const handleCloseHttpLogsConfig = () => {
    setIsHttpLogsConfigOpen(false)
  }

  const handleHttpLogsConfigSave = () => {
    fetchHttpLogState()
  }

  const handleOpenLocalURL = () => {
    const baseUrl = config.domain_enabled ? config.alias : config.local_address

    openShell(`http://${baseUrl}:${config.local_port}`).catch(console.error)
  }

  const togglePortForwarding = async (isChecked: boolean) => {
    setIsInitiating(true)
    setLocalInitiating(true)
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
      setLocalInitiating(false)
    }
  }

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
      toaster.error({
        title: 'Error starting port forwarding',
        description: error instanceof Error ? error.message : String(error),
        duration: 1000,
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

  const handleCopyPodName = async () => {
    if (activePod) {
      try {
        await navigator.clipboard.writeText(activePod)
        toaster.success({
          title: 'Pod name copied',
          description: `${activePod} copied to clipboard`,
          duration: 1000,
        })
      } catch (error) {
        console.error('Failed to copy pod name:', error)
        toaster.error({
          title: 'Copy failed',
          description: 'Failed to copy pod name to clipboard',
          duration: 1000,
        })
      }
    }
  }

  const getStatusInfo = () => {
    // Follow same logic as checkbox: config.is_running
    if (config.is_running) {
      // Orange: Running but has specific issues
      if (activePod && activePod.includes('pending-rollout')) {
        return {
          color: 'rgba(161, 98, 7, 0.7)',
          status: 'Rollout',
          description: 'Pod rollout in progress',
        }
      }

      // Blue: Running (default for any running state)
      const status = localInitiating
        ? config.is_running
          ? 'Stopping'
          : 'Starting'
        : activePod
          ? 'Running'
          : 'Pending'

      const description = localInitiating
        ? config.is_running
          ? 'Port forward is stopping...'
          : 'Port forward is starting...'
        : activePod
          ? `Connected to ${activePod}`
          : 'Waiting for healthy pod...'

      return {
        color: 'rgba(59, 130, 246, 0.8)',
        status,
        description,
      }
    }

    // Gray: Stopped
    return {
      color: 'rgba(100, 116, 139, 0.4)',
      status: 'Stopped',
      description: 'Port forward is stopped',
    }
  }

  return (
    <>
      <Table.Row className='table-row'>
        <Table.Cell className='table-cell'>
          <Flex align='center' gap={1.5}>
            <Tooltip
              content={
                <Box p={1}>
                  <Text fontSize='xs' fontWeight='medium'>
                    Status: {getStatusInfo().status}
                  </Text>
                  <Box
                    borderTop='1px solid'
                    borderColor='rgba(255,255,255,0.1)'
                    pt={1}
                    mt={1}
                  >
                    <Text fontSize='xs'>
                      <strong>Alias:</strong> {config.alias}
                    </Text>
                    <Text fontSize='xs'>
                      <strong>Workload:</strong> {config.workload_type}
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
                      <strong>Protocol:</strong> {config.protocol}
                    </Text>
                  </Box>
                </Box>
              }
              portalled
            >
              <Flex align='center' gap={1.5}>
                <Checkbox
                  size='xs'
                  checked={selected}
                  onCheckedChange={e => {
                    onSelectionChange(e.checked === true)
                  }}
                  className='checkbox'
                />

                <IconButton
                  size='xs'
                  variant='ghost'
                  aria-label='Info'
                  className='icon-button'
                  style={{ color: getStatusInfo().color }}
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
              checked={config.is_running}
              onCheckedChange={details => togglePortForwarding(details.checked)}
              disabled={localInitiating}
              data-loading={localInitiating ? '' : undefined}
              unstyled={true}
              className='switch'
            />

            <Flex
              align='center'
              gap={0.5}
              mr={-10}
              minWidth='48px'
              justifyContent='flex-end'
            >
              {config.is_running ? (
                <Tooltip content='Open in browser' portalled>
                  <IconButton
                    size='2xs'
                    variant='ghost'
                    aria-label='Open URL'
                    onClick={handleOpenLocalURL}
                    className='icon-button'
                  >
                    <ExternalLinkIcon size={10} />
                  </IconButton>
                </Tooltip>
              ) : (
                <Box width='24px' height='24px' />
              )}

              {config.is_running && activePod ? (
                <Tooltip
                  content={
                    <Box p={1}>
                      <Text fontSize='xs' fontWeight='medium'>
                        Status: {getStatusInfo().status}
                      </Text>
                      <Text fontSize='xs' color='gray.400'>
                        Pod: {activePod} (click to copy)
                      </Text>
                    </Box>
                  }
                  portalled
                >
                  <IconButton
                    size='2xs'
                    variant='ghost'
                    onClick={handleCopyPodName}
                    aria-label='Copy Pod Name'
                    className='icon-button'
                  >
                    <ClipboardIcon size={10} />
                  </IconButton>
                </Tooltip>
              ) : (
                <Box width='24px' height='24px' />
              )}
            </Flex>
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
                <>
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
                  {httpLogsEnabled[config.id] && (
                    <MenuItem
                      className='menu-item'
                      value='open-http-logs'
                      onClick={handleInspectLogs}
                    >
                      <FileIcon size={12} />
                      <Text ml={2} fontSize='xs'>
                        Open HTTP Logs File
                      </Text>
                    </MenuItem>
                  )}
                  <MenuItem
                    className='menu-item'
                    value='http-logs-config'
                    onClick={handleOpenHttpLogsConfig}
                  >
                    <SettingsIcon size={12} />
                    <Text ml={2} fontSize='xs'>
                      HTTP Logs Settings
                    </Text>
                  </MenuItem>
                </>
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

      <HttpLogsConfigModal
        configId={config.id}
        isOpen={isHttpLogsConfigOpen}
        onClose={handleCloseHttpLogsConfig}
        onSave={handleHttpLogsConfigSave}
      />
    </>
  )
}

export default PortForwardRow
