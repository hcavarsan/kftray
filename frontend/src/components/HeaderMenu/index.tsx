import type { Dispatch, SetStateAction } from 'react'
import { useMemo } from 'react'
import { ChevronsDownUp, ChevronsUpDown, RefreshCw, X } from 'lucide-react'

import { Box, Group } from '@chakra-ui/react'

import { ForwardBatchButton } from '@/components/HeaderMenu/ForwardBatchButton'
import {
  ToolbarIconButton,
  WithTooltip,
} from '@/components/HeaderMenu/ToolbarParts'
import { ViewControls } from '@/components/HeaderMenu/ViewControls'
import { Checkbox } from '@/components/ui/checkbox'
import type { Config, ConfigView, Facet } from '@/types'

interface HeaderMenuProps {
  configs: Config[]
  selectedConfigs: Config[]
  initiatePortForwarding: (configs: Config[]) => void
  startSelectedPortForwarding: () => void
  stopSelectedPortForwarding: () => void
  stopAllPortForwarding: () => void
  abortStartOperation: () => void
  abortStopOperation: () => void
  isInitiating: boolean
  isStopping: boolean
  toggleExpandAll: () => void
  expandedIndices: string[]
  groupCount: number
  view: ConfigView | null
  facets: Facet[]
  setView: (view: ConfigView) => void
  setSelectedConfigs: Dispatch<SetStateAction<Config[]>>
}

export function HeaderMenu({
  configs,
  selectedConfigs,
  initiatePortForwarding,
  startSelectedPortForwarding,
  stopSelectedPortForwarding,
  stopAllPortForwarding,
  abortStartOperation,
  abortStopOperation,
  isInitiating,
  isStopping,
  toggleExpandAll,
  expandedIndices,
  groupCount,
  view,
  facets,
  setView,
  setSelectedConfigs,
}: HeaderMenuProps) {
  const isAllExpanded = expandedIndices.length === groupCount
  const expandLabel = isAllExpanded
    ? 'Collapse all groups'
    : 'Expand all groups'

  const selectedIds = useMemo(
    () => new Set(selectedConfigs.map(config => config.id)),
    [selectedConfigs],
  )
  const configById = useMemo(
    () => new Map(configs.map(config => [config.id, config])),
    [configs],
  )

  const isSelectAllChecked = useMemo(
    () =>
      configs.length > 0 && configs.every(config => selectedIds.has(config.id)),
    [configs, selectedIds],
  )

  const hasSelectedNotRunning = useMemo(
    () =>
      selectedConfigs.some(
        selected => configById.get(selected.id)?.is_running === false,
      ),
    [selectedConfigs, configById],
  )
  const hasSelectedRunning = useMemo(
    () =>
      selectedConfigs.some(
        selected => configById.get(selected.id)?.is_running === true,
      ),
    [selectedConfigs, configById],
  )

  const handleCheckboxChange = ({
    checked,
  }: {
    checked: boolean | 'indeterminate'
  }) => {
    setSelectedConfigs(checked === true ? configs : [])
  }

  const isStartBusy =
    isInitiating ||
    (selectedConfigs.length > 0
      ? selectedConfigs.every(
          selected => configById.get(selected.id)?.is_running === true,
        )
      : configs.every(config => config.is_running))

  const isStopBusy =
    isStopping ||
    (selectedConfigs.length > 0
      ? selectedConfigs.every(
          selected => configById.get(selected.id)?.is_running === false,
        )
      : configs.every(config => !config.is_running))

  return (
    <Box
      display='flex'
      alignItems='center'
      justifyContent='space-between'
      width='100%'
      bg='bg.surface'
      px={3}
      py={3}
      borderTopRadius='none'
      borderTop='none'
      borderBottomRadius='lg'
      border='1px solid'
      borderColor='border'
      position='relative'
      zIndex={10}
      borderTopColor='border.subtle'
      mt='-1px'
    >
      <Group display='flex' alignItems='center' gap={3}>
        <Checkbox
          ml={2}
          size='sm'
          checked={isSelectAllChecked}
          onCheckedChange={handleCheckboxChange}
          css={{
            '& input': {
              width: '10px',
              height: '10px',
              background: 'bg.raised',
              border: '1px solid',
              borderColor: 'border.emphasized',
              borderRadius: '3px',
              '&:hover': {
                borderColor: 'border.strong',
              },
            },
            '& input:checked': {
              background: 'accent.solid',
              borderColor: 'accent.solid',
            },
          }}
        />

        <Group display='flex' alignItems='center' gap={2}>
          <ForwardBatchButton
            icon={RefreshCw}
            label={
              selectedConfigs.length > 0 && hasSelectedNotRunning
                ? 'Start Selected'
                : 'Start All'
            }
            busyLabel='Starting...'
            tooltip={
              isInitiating
                ? 'Starting port forwards...'
                : selectedConfigs.length > 0 && hasSelectedNotRunning
                  ? 'Start selected port forwards'
                  : 'Start all port forwards'
            }
            isPending={isInitiating}
            disabled={isStartBusy}
            onClick={
              selectedConfigs.length > 0
                ? () => void startSelectedPortForwarding()
                : () =>
                    void initiatePortForwarding(
                      configs.filter(config => !config.is_running),
                    )
            }
            onCancel={abortStartOperation}
          />

          <ForwardBatchButton
            icon={X}
            label={
              selectedConfigs.length > 0 && hasSelectedRunning
                ? 'Stop Selected'
                : 'Stop All'
            }
            busyLabel='Stopping...'
            tooltip={
              isStopping
                ? 'Stopping port forwards...'
                : selectedConfigs.length > 0 && hasSelectedRunning
                  ? 'Stop selected port forwards'
                  : 'Stop all port forwards'
            }
            isPending={isStopping}
            disabled={isStopBusy}
            onClick={
              selectedConfigs.length > 0
                ? () => void stopSelectedPortForwarding()
                : () => void stopAllPortForwarding()
            }
            onCancel={abortStopOperation}
          />
        </Group>
      </Group>

      <Group display='flex' alignItems='center' gap={1.5}>
        <ViewControls view={view} facets={facets} setView={setView} />
        <WithTooltip content={expandLabel}>
          <ToolbarIconButton aria-label={expandLabel} onClick={toggleExpandAll}>
            {isAllExpanded ? (
              <ChevronsDownUp size={13} />
            ) : (
              <ChevronsUpDown size={13} />
            )}
          </ToolbarIconButton>
        </WithTooltip>
      </Group>
    </Box>
  )
}
