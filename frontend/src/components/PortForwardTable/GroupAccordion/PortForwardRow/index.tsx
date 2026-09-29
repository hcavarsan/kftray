import { memo, useState } from 'react'
import { ClipboardIcon, ExternalLinkIcon, Info } from 'lucide-react'

import { Box, Flex, IconButton, Table, Text } from '@chakra-ui/react'
import { useQueryClient } from '@tanstack/react-query'
import { invoke } from '@tauri-apps/api/core'
import { open as openShell } from '@tauri-apps/plugin-shell'

import HttpLogsConfigModal from '@/components/HttpLogsConfigModal'
import { ActionsMenu } from '@/components/PortForwardTable/GroupAccordion/PortForwardRow/ActionsMenu'
import { ConfigDetailsTooltip } from '@/components/PortForwardTable/GroupAccordion/PortForwardRow/ConfigDetailsTooltip'
import { DeleteConfigDialog } from '@/components/PortForwardTable/GroupAccordion/PortForwardRow/DeleteConfigDialog'
import {
  formatConfigDetails,
  getConfigDetails,
  getStatusInfo,
} from '@/components/PortForwardTable/GroupAccordion/PortForwardRow/statusInfo'
import {
  httpLogsEnabledQueryKey,
  useHttpLogsEnabled,
  useSetHttpLogsEnabled,
} from '@/components/PortForwardTable/GroupAccordion/PortForwardRow/useHttpLogsEnabled'
import { Checkbox } from '@/components/ui/checkbox'
import { Switch } from '@/components/ui/switch'
import { toaster } from '@/components/ui/toaster'
import { Tooltip } from '@/components/ui/tooltip'
import { errorMessage } from '@/lib/errors'
import type {
  Config,
  PendingConfigAction,
  PortForwardToggleAction,
} from '@/types'

import '../../styles.css'

const HTTP_URL_PATTERN = /^https?:\/\//i

function openConfigUrl(url: string) {
  if (!HTTP_URL_PATTERN.test(url)) {
    return
  }

  openShell(url).catch(error => {
    toaster.error({
      title: 'Failed to open URL',
      description: errorMessage(error),
    })
  })
}

function buildLocalUrl(config: Config): string {
  if (config.workload_type === 'expose') {
    if (config.exposure_type === 'public') {
      const protocol = config.cert_manager_enabled ? 'https' : 'http'

      return `${protocol}://${config.alias}`
    }

    const host = config.local_address || '127.0.0.1'

    return `http://${host}:${config.local_port}`
  }

  const baseUrl = config.domain_enabled
    ? config.alias
    : config.local_address || 'localhost'

  return `http://${baseUrl}:${config.local_port}`
}

interface PortForwardRowProps {
  config: Config
  deleteConfigs: (ids: number[]) => Promise<boolean>
  handleEditConfig: (id: number) => Promise<void>
  handleDuplicateConfig: (id: number) => Promise<void>
  onSelectionChange: (id: number, isSelected: boolean) => void
  selected: boolean
  pendingAction: PendingConfigAction | null
  activePod: string | null
  toggleConfigForward: (
    config: Config,
    action: PortForwardToggleAction,
  ) => Promise<void>
}

function PortForwardRowComponent({
  config,
  deleteConfigs,
  handleEditConfig,
  handleDuplicateConfig,
  selected,
  onSelectionChange,
  pendingAction,
  activePod,
  toggleConfigForward,
}: PortForwardRowProps) {
  const [isDeleteDialogOpen, setIsDeleteDialogOpen] = useState(false)
  const [isHttpLogsConfigOpen, setIsHttpLogsConfigOpen] = useState(false)
  const isPending = pendingAction !== null

  const queryClient = useQueryClient()
  const { data: httpLogsEnabled } = useHttpLogsEnabled(config.id)
  const setHttpLogsEnabled = useSetHttpLogsEnabled(config.id)

  const status = getStatusInfo(config, pendingAction, activePod)
  const configDetails = getConfigDetails(config, activePod)

  const handleInspectLogs = async () => {
    try {
      await invoke('open_log_file', {
        logFileName: `${config.id}_${config.local_port}.http`,
      })
    } catch (error) {
      toaster.error({
        title: 'Error opening log file',
        description: errorMessage(error),
      })
    }
  }

  const togglePortForwarding = async (isChecked: boolean) => {
    await toggleConfigForward(config, isChecked ? 'starting' : 'stopping')
  }

  const handleCopyPodName = async () => {
    if (!activePod) {
      return
    }

    try {
      await navigator.clipboard.writeText(activePod)
      toaster.success({
        title: 'Pod name copied',
        description: `${activePod} copied to clipboard`,
        duration: 1000,
      })
    } catch {
      toaster.error({
        title: 'Copy failed',
        description: 'Failed to copy pod name to clipboard',
        duration: 1000,
      })
    }
  }

  const handleCopyConfigDetails = async () => {
    try {
      await navigator.clipboard.writeText(
        formatConfigDetails(status, configDetails),
      )
      toaster.success({
        title: 'Config details copied',
        description: 'All configuration details copied to clipboard',
        duration: 1000,
      })
    } catch {
      toaster.error({
        title: 'Copy failed',
        description: 'Failed to copy config details to clipboard',
        duration: 1000,
      })
    }
  }

  const handleDeleteConfirm = async () => {
    const success = await deleteConfigs([config.id])

    if (success) {
      setIsDeleteDialogOpen(false)
    }
  }

  return (
    <>
      <Table.Row className='table-row'>
        <Table.Cell className='table-cell'>
          <Flex align='center' gap={1.5}>
            <Tooltip
              content={
                <ConfigDetailsTooltip status={status} details={configDetails} />
              }
              portalled
            >
              <Flex align='center' gap={1.5}>
                <Checkbox
                  size='xs'
                  checked={selected}
                  onCheckedChange={e =>
                    onSelectionChange(config.id, e.checked === true)
                  }
                  className='checkbox'
                />

                <IconButton
                  size='xs'
                  variant='ghost'
                  aria-label='Info'
                  onClick={() => void handleCopyConfigDetails()}
                  className='icon-button'
                  style={{ color: status.color }}
                >
                  <Box as={Info} width='12px' height='12px' />
                </IconButton>

                <Text className='text-normal' truncate maxWidth='100%'>
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
              onCheckedChange={state =>
                void togglePortForwarding(state.checked)
              }
              disabled={isPending}
              data-loading={isPending ? '' : undefined}
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
                    onClick={() => openConfigUrl(buildLocalUrl(config))}
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
                        Status: {status.status}
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
                    onClick={() => void handleCopyPodName()}
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
          <ActionsMenu
            protocol={config.protocol}
            isPending={isPending}
            httpLogsEnabled={httpLogsEnabled}
            onEdit={() => void handleEditConfig(config.id)}
            onDuplicate={() => void handleDuplicateConfig(config.id)}
            onOpenDeleteDialog={() => setIsDeleteDialogOpen(true)}
            onToggleHttpLogs={() => setHttpLogsEnabled.mutate(!httpLogsEnabled)}
            onInspectLogs={() => void handleInspectLogs()}
            onOpenHttpLogsConfig={() => setIsHttpLogsConfigOpen(true)}
          />
        </Table.Cell>
      </Table.Row>

      {isDeleteDialogOpen && (
        <DeleteConfigDialog
          isPending={isPending}
          onConfirm={() => void handleDeleteConfirm()}
          onClose={() => setIsDeleteDialogOpen(false)}
        />
      )}

      {isHttpLogsConfigOpen && (
        <HttpLogsConfigModal
          configId={config.id}
          onClose={() => setIsHttpLogsConfigOpen(false)}
          onSaved={() =>
            queryClient.invalidateQueries({
              queryKey: httpLogsEnabledQueryKey(config.id),
            })
          }
        />
      )}
    </>
  )
}

export default memo(PortForwardRowComponent)
