import type { Dispatch, SetStateAction } from 'react'
import { useMemo } from 'react'
import {
  ChevronsDownUp,
  ChevronsUpDown,
  Loader2,
  RefreshCw,
  X,
} from 'lucide-react'

import { Box, chakra, Group } from '@chakra-ui/react'

import ViewControls, {
  ToolbarIconButton,
  WithTooltip,
} from '@/components/HeaderMenu/ViewControls'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { Tooltip } from '@/components/ui/tooltip'
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

function HeaderMenu({
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
      bg='app.panel'
      px={3}
      py={3}
      borderTopRadius='none'
      borderTop='none'
      borderBottomRadius='lg'
      border='1px solid'
      borderColor='app.border'
      position='relative'
      zIndex={10}
      borderTopColor='app.faint'
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
              background: 'app.raised',
              border: '1px solid',
              borderColor: 'app.borderStrong',
              borderRadius: '3px',
              '&:hover': {
                borderColor:
                  'color-mix(in srgb, var(--chakra-colors-white) 25%, transparent)',
              },
            },
            '& input:checked': {
              background: '#3182CE',
              borderColor: '#3182CE',
            },
          }}
        />

        <Group display='flex' alignItems='center' gap={2}>
          <Tooltip
            content={
              isInitiating
                ? 'Starting port forwards...'
                : selectedConfigs.length > 0 && hasSelectedNotRunning
                  ? 'Start selected port forwards'
                  : 'Start all port forwards'
            }
            portalled={true}
            contentProps={{ zIndex: 100 }}
          >
            <Button
              size='xs'
              variant='ghost'
              disabled={isStartBusy}
              onClick={
                selectedConfigs.length > 0
                  ? () => void startSelectedPortForwarding()
                  : () =>
                      void initiatePortForwarding(
                        configs.filter(config => !config.is_running),
                      )
              }
              _hover={{ bg: isInitiating ? undefined : 'whiteAlpha.100' }}
              _disabled={{ cursor: 'not-allowed' }}
              height='26px'
              minWidth='90px'
              bg='whiteAlpha.50'
              px={2}
              borderRadius='md'
              border='1px solid'
              borderColor='app.border'
            >
              {isInitiating ? (
                <>
                  <Box
                    as={Loader2}
                    width='12px'
                    height='12px'
                    marginRight={1.5}
                    animation='spin 1s linear infinite'
                    css={{
                      '@keyframes spin': {
                        from: { transform: 'rotate(0deg)' },
                        to: { transform: 'rotate(360deg)' },
                      },
                    }}
                  />
                  <span style={{ fontSize: '11px' }}>Starting...</span>
                </>
              ) : (
                <>
                  <Box
                    as={RefreshCw}
                    width='12px'
                    height='12px'
                    marginRight={1.5}
                  />
                  <span style={{ fontSize: '11px' }}>
                    {selectedConfigs.length > 0 && hasSelectedNotRunning
                      ? 'Start Selected'
                      : 'Start All'}
                  </span>
                </>
              )}
            </Button>
          </Tooltip>

          {isInitiating && (
            <Tooltip content='Cancel' portalled contentProps={{ zIndex: 101 }}>
              <chakra.button
                type='button'
                aria-label='Cancel'
                display='inline-flex'
                alignItems='center'
                justifyContent='center'
                padding='6px'
                borderRadius='sm'
                cursor='pointer'
                bg='transparent'
                border='none'
                _hover={{ bg: 'red.700' }}
                onClick={() => abortStartOperation()}
              >
                <Box as={X} width='10px' height='10px' color='red.300' />
              </chakra.button>
            </Tooltip>
          )}

          <Tooltip
            content={
              isStopping
                ? 'Stopping port forwards...'
                : selectedConfigs.length > 0 && hasSelectedRunning
                  ? 'Stop selected port forwards'
                  : 'Stop all port forwards'
            }
            portalled={true}
            contentProps={{ zIndex: 100 }}
          >
            <Button
              size='xs'
              variant='ghost'
              disabled={isStopBusy}
              onClick={
                selectedConfigs.length > 0
                  ? () => void stopSelectedPortForwarding()
                  : () => void stopAllPortForwarding()
              }
              _hover={{ bg: isStopping ? undefined : 'whiteAlpha.100' }}
              _disabled={{ cursor: 'not-allowed' }}
              height='26px'
              minWidth='90px'
              bg='whiteAlpha.50'
              px={2}
              borderRadius='md'
              border='1px solid'
              borderColor='app.border'
            >
              {isStopping ? (
                <>
                  <Box
                    as={Loader2}
                    width='12px'
                    height='12px'
                    marginRight={1.5}
                    animation='spin 1s linear infinite'
                    css={{
                      '@keyframes spin': {
                        from: { transform: 'rotate(0deg)' },
                        to: { transform: 'rotate(360deg)' },
                      },
                    }}
                  />
                  <span style={{ fontSize: '11px' }}>Stopping...</span>
                </>
              ) : (
                <>
                  <Box as={X} width='12px' height='12px' marginRight={1.5} />
                  <span style={{ fontSize: '11px' }}>
                    {selectedConfigs.length > 0 && hasSelectedRunning
                      ? 'Stop Selected'
                      : 'Stop All'}
                  </span>
                </>
              )}
            </Button>
          </Tooltip>

          {isStopping && (
            <Tooltip content='Cancel' portalled contentProps={{ zIndex: 101 }}>
              <chakra.button
                type='button'
                aria-label='Cancel'
                display='inline-flex'
                alignItems='center'
                justifyContent='center'
                padding='6px'
                borderRadius='sm'
                cursor='pointer'
                bg='transparent'
                border='none'
                _hover={{ bg: 'red.700' }}
                onClick={() => abortStopOperation()}
              >
                <Box as={X} width='10px' height='10px' color='red.300' />
              </chakra.button>
            </Tooltip>
          )}
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

export default HeaderMenu
