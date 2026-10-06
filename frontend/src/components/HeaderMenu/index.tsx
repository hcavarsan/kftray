import type { Dispatch, SetStateAction } from 'react'
import { useMemo } from 'react'
import { ChevronsDownUp, ChevronsUpDown, RefreshCw, X } from 'lucide-react'

import { Box, Group } from '@chakra-ui/react'

import { ForwardBatchButton } from '@/components/HeaderMenu/ForwardBatchButton'
import { forwardBatchState } from '@/components/HeaderMenu/forwardBatchState'
import {
  ToolbarIconButton,
  WithTooltip,
} from '@/components/HeaderMenu/ToolbarParts'
import { ViewControls } from '@/components/HeaderMenu/ViewControls'
import { Checkbox } from '@/components/ui/checkbox'
import type { Config, ConfigView, Facet } from '@/types'

interface HeaderMenuProps {
  configs: Config[]
  allConfigs: Config[]
  selectedConfigs: Config[]
  initiatePortForwarding: (configs: Config[]) => void
  startSelectedPortForwarding: (configs: Config[]) => void
  stopSelectedPortForwarding: (configs: Config[]) => void
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
  allConfigs,
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
  const isSelectAllChecked = useMemo(
    () =>
      configs.length > 0 && configs.every(config => selectedIds.has(config.id)),
    [configs, selectedIds],
  )
  const batch = useMemo(
    () => forwardBatchState(configs, selectedConfigs, allConfigs),
    [configs, selectedConfigs, allConfigs],
  )

  const handleCheckboxChange = ({
    checked,
  }: {
    checked: boolean | 'indeterminate'
  }) => {
    setSelectedConfigs(checked === true ? configs : [])
  }

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
          ml='9px'
          size='xs'
          checked={isSelectAllChecked}
          onCheckedChange={handleCheckboxChange}
          inputProps={{ 'aria-label': 'Select all visible configurations' }}
        />

        <Group display='flex' alignItems='center' gap={2}>
          <ForwardBatchButton
            icon={RefreshCw}
            label={batch.startSelected ? 'Start Selected' : 'Start All'}
            busyLabel='Starting...'
            tooltip={
              isInitiating
                ? 'Starting port forwards...'
                : batch.startSelected
                  ? 'Start selected port forwards'
                  : 'Start all port forwards'
            }
            isPending={isInitiating}
            disabled={isInitiating || batch.startDisabled}
            onClick={
              batch.startSelected
                ? () => void startSelectedPortForwarding(batch.selected)
                : () =>
                    void initiatePortForwarding(
                      configs.filter(config => !config.is_running),
                    )
            }
            onCancel={abortStartOperation}
          />

          <ForwardBatchButton
            icon={X}
            label={batch.stopSelected ? 'Stop Selected' : 'Stop All'}
            busyLabel='Stopping...'
            tooltip={
              isStopping
                ? 'Stopping port forwards...'
                : batch.stopSelected
                  ? 'Stop selected port forwards'
                  : 'Stop all port forwards'
            }
            isPending={isStopping}
            disabled={isStopping || batch.stopDisabled}
            onClick={
              batch.stopSelected
                ? () => void stopSelectedPortForwarding(batch.selected)
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
