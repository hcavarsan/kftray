import React, { useCallback, useEffect, useRef, useState } from 'react'
import { ExternalLinkIcon } from 'lucide-react'

import {
  Box,
  Button,
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
  selected,
  onSelectionChange,
  isInitiating,
  setIsInitiating,
}) => {
  const toast = useCustomToast()
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
      const errorMessage =
        error instanceof Error ? error.message : String(error)

      toast({
        title: 'Error toggling HTTP logs',
        description: errorMessage,
        status: 'error',
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
      const errorMessage =
        error instanceof Error ? error.message : String(error)

      toast({
        title: 'Error stopping port forwarding',
        description: errorMessage,
        status: 'error',
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
    <Table.Row
      key={config.id}
      css={{
        height: '40px',
        width: '100%',
        borderBottom: '1px solid rgba(255, 255, 255, 0.06)',
        transition: 'background-color 0.2s',
        '&:hover': {
          bg: 'rgba(255, 255, 255, 0.02)',
        },
      }}
    >
      <Table.Cell py={1}>
        <Flex align='center' gap={2}>
          <Checkbox
            size='xs'
            checked={selected || config.is_running}
            onChange={() => onSelectionChange(!selected)}
            disabled={config.is_running}
          />
          <Text color='gray.300' fontSize='xs'>
            {config.alias}
          </Text>
          <Tooltip content={getTooltipContent(config)}>
            <IconButton
              size='xs'
              variant='ghost'
              aria-label='Info'
              css={{
                minWidth: '20px',
                height: '20px',
                color: 'gray.400',
                _hover: { color: 'gray.200' },
              }}
            >
              <FontAwesomeIcon icon={faInfoCircle} size='xs' />
            </IconButton>
          </Tooltip>
        </Flex>
      </Table.Cell>

      <Table.Cell py={1}>
        <Text color='gray.300' fontSize='xs'>
          {config.local_port}
        </Text>
      </Table.Cell>

      <Table.Cell py={1}>
        <Flex align='center' gap={2}>
          <Switch
            size='sm'
            checked={config.is_running && !isInitiating}
            onCheckedChange={details => togglePortForwarding(details.checked)}
            disabled={isInitiating}
            css={{
              '& span': {
                bg: config.is_running ? 'green.500' : 'gray.600',
              },
            }}
          />
          {config.is_running && (
            <IconButton
              size='xs'
              variant='ghost'
              aria-label='Open URL'
              onClick={handleOpenLocalURL}
              css={{
                minWidth: '20px',
                height: '20px',
                color: 'blue.400',
                _hover: { color: 'blue.300' },
              }}
            >
              <ExternalLinkIcon size={12} />
            </IconButton>
          )}
        </Flex>
      </Table.Cell>

      <Table.Cell py={1}>
        <MenuRoot>
          <MenuTrigger asChild>
            <IconButton
              size='xs'
              variant='ghost'
              aria-label='Actions'
              css={{
                minWidth: '20px',
                height: '20px',
                color: 'gray.400',
                _hover: { color: 'gray.200' },
              }}
            >
              <FontAwesomeIcon icon={faBars} size='xs' />
            </IconButton>
          </MenuTrigger>
          <MenuContent>
            <MenuItem value='edit' onClick={() => handleEditConfig(config.id)}>
              <FontAwesomeIcon icon={faPen} size='xs' />
              <Text ml={2} fontSize='xs'>
                Edit
              </Text>
            </MenuItem>
            <MenuItem value='delete' onClick={handleOpenDeleteDialog}>
              <FontAwesomeIcon icon={faTrash} size='xs' />
              <Text ml={2} fontSize='xs'>
                Delete
              </Text>
            </MenuItem>
            {config.protocol === 'tcp' && (
              <MenuItem value='http-logs' onClick={handleToggleHttpLogs}>
                <FontAwesomeIcon icon={faFileAlt} size='xs' />
                <Text ml={2} fontSize='xs'>
                  {httpLogsEnabled[config.id] ? 'Disable' : 'Enable'} HTTP Logs
                </Text>
              </MenuItem>
            )}
          </MenuContent>
        </MenuRoot>

        {isDeleteDialogOpen && (
          <DialogRoot
            role='alertdialog'
            open={isDeleteDialogOpen}
            onOpenChange={handleOpenChange}
          >
            <DialogContent
              css={{
                backgroundColor: '#1A1A1A',
                border: '1px solid rgba(255, 255, 255, 0.08)',
                padding: '16px',
                zIndex: 1000,
                position: 'fixed',
                top: '50%',
                left: '50%',
                transform: 'translate(-50%, -50%)',
              }}
            >
              <DialogHeader>
                <DialogTitle fontSize='11px' fontWeight='bold'>
                  Delete Configuration
                </DialogTitle>
              </DialogHeader>
              <DialogBody fontSize='11px' py={4}>
                Are you sure? You can&lsquo;t undo this action afterwards.
              </DialogBody>
              <DialogFooter>
                <DialogCloseTrigger asChild>
                  <Button
                    size='2xs'
                    variant='ghost'
                    height='24px'
                    minWidth='60px'
                    bg='whiteAlpha.50'
                    _hover={{ bg: 'whiteAlpha.100' }}
                  >
                    <span style={{ fontSize: '11px' }}>Cancel</span>
                  </Button>
                </DialogCloseTrigger>
                <Button
                  size='2xs'
                  variant='ghost'
                  onClick={() => {
                    confirmDeleteConfig()
                    setIsDeleteDialogOpen(false)
                  }}
                  height='24px'
                  minWidth='60px'
                  bg='red.500'
                  _hover={{ bg: 'red.600' }}
                  ml={2}
                >
                  <span style={{ fontSize: '11px' }}>Delete</span>
                </Button>
              </DialogFooter>
            </DialogContent>
          </DialogRoot>
        )}
      </Table.Cell>
    </Table.Row>
  )
}

const getTooltipContent = (config: any) => (
  <Box p={1.5}>
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
