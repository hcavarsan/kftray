import type { Dispatch, SetStateAction } from 'react'
import { useCallback, useEffect, useMemo, useState } from 'react'

import { Box, Button, Flex, Text } from '@chakra-ui/react'

import { Header } from '@/components/Header'
import { HeaderMenu } from '@/components/HeaderMenu'
import { GroupAccordion } from '@/components/PortForwardTable/GroupAccordion'
import {
  NO_PODS,
  useActivePods,
  useGroupActivePods,
} from '@/components/PortForwardTable/useActivePods'
import {
  ALL_GROUP_ID,
  useConfigView,
} from '@/components/PortForwardTable/useConfigView'
import {
  AccordionRoot,
  type ValueChangeDetails,
} from '@/components/ui/accordion'
import type {
  Config,
  PendingConfigAction,
  PortForwardToggleAction,
  ResolvedGroup,
} from '@/types'

interface TableProps {
  configs: Config[]
  isInitiating: boolean
  isStopping: boolean
  pendingConfigActions: Map<number, PendingConfigAction>
  toggleConfigForward: (
    config: Config,
    action: PortForwardToggleAction,
  ) => Promise<void>
  initiatePortForwarding: (configs: Config[]) => Promise<void>
  startSelectedPortForwarding: () => Promise<void>
  stopSelectedPortForwarding: () => Promise<void>
  stopAllPortForwarding: () => Promise<void>
  abortStartOperation: () => void
  abortStopOperation: () => void
  deleteConfigs: (ids: number[]) => Promise<boolean>
  handleEditConfig: (id: number) => Promise<void>
  handleDuplicateConfig: (id: number) => Promise<void>
  selectedConfigs: Config[]
  setSelectedConfigs: Dispatch<SetStateAction<Config[]>>
}

export function PortForwardTable({
  configs,
  isInitiating,
  isStopping,
  pendingConfigActions,
  toggleConfigForward,
  initiatePortForwarding,
  startSelectedPortForwarding,
  stopSelectedPortForwarding,
  stopAllPortForwarding,
  abortStartOperation,
  abortStopOperation,
  handleEditConfig,
  handleDuplicateConfig,
  deleteConfigs,
  selectedConfigs,
  setSelectedConfigs,
}: TableProps) {
  const [search, setSearch] = useState<string>('')
  const [expandedIndices, setExpandedIndices] = useState<string[]>([])
  const [isCheckboxAction, setIsCheckboxAction] = useState<boolean>(false)

  const selectedIds = useMemo(
    () => new Set(selectedConfigs.map(config => config.id)),
    [selectedConfigs],
  )
  const activePods = useActivePods(configs)

  const filteredConfigs = useMemo(() => {
    const searchLower = search.toLowerCase()

    return configs
      .filter(
        config =>
          (config.alias ?? '').toLowerCase().includes(searchLower) ||
          (config.context ?? '').toLowerCase().includes(searchLower) ||
          (config.workload_type === 'proxy' &&
            config.remote_address.toLowerCase().includes(searchLower)) ||
          String(config.local_port ?? '').includes(searchLower) ||
          Object.entries(config.tags ?? {}).some(([key, value]) =>
            `${key}=${value}`.toLowerCase().includes(searchLower),
          ),
      )
      .sort(
        (a, b) =>
          (a.alias ?? '').localeCompare(b.alias ?? '') ||
          (a.context ?? '').localeCompare(b.context ?? ''),
      )
  }, [configs, search])

  const { view, facets, groups, setView } = useConfigView(
    configs,
    filteredConfigs,
  )
  const groupPods = useGroupActivePods(groups, activePods)
  const groupBy = view?.group_by
  const visibleConfigs = useMemo(
    () => groups.flatMap(group => group.configs),
    [groups],
  )

  useEffect(() => {
    setExpandedIndices(groupBy === null ? [ALL_GROUP_ID] : [])
  }, [groupBy])

  useEffect(() => {
    setSelectedConfigs(prev => {
      const next = prev
        .map(selected => configs.find(c => c.id === selected.id))
        .filter((config): config is Config => config !== undefined)

      if (
        next.length === prev.length &&
        next.every((config, index) => config.id === prev[index].id)
      ) {
        return prev
      }

      return next
    })
  }, [configs, setSelectedConfigs])

  const toggleExpandAll = () => {
    const allGroups = groups.map(group => group.id)

    setExpandedIndices(current =>
      current.length === allGroups.length ? [] : allGroups,
    )
  }

  const handleAccordionChange = (details: ValueChangeDetails) => {
    if (!isCheckboxAction) {
      setExpandedIndices(details.value)
    }
  }

  const handleCheckboxChange = useCallback(
    (group: ResolvedGroup, isChecked: boolean) => {
      setIsCheckboxAction(true)
      const groupIds = new Set(group.configs.map(config => config.id))

      setSelectedConfigs(prev => {
        if (isChecked) {
          const selectedInGroup = new Set(prev.map(config => config.id))

          return [
            ...prev,
            ...group.configs.filter(config => !selectedInGroup.has(config.id)),
          ]
        }

        return prev.filter(config => !groupIds.has(config.id))
      })
      setIsCheckboxAction(false)
    },
    [setSelectedConfigs],
  )

  const handleSelectionChange = useCallback(
    (config: Config, isSelected: boolean) => {
      setSelectedConfigs(prev => {
        if (!isSelected) {
          return prev.filter(c => c.id !== config.id)
        }
        if (prev.some(c => c.id === config.id)) {
          return prev
        }

        return [...prev, config]
      })
    },
    [setSelectedConfigs],
  )

  return (
    <Box
      display='flex'
      flexDirection='column'
      height='100%'
      width='100%'
      overflow='hidden'
      bg='transparent'
      position='relative'
    >
      <Box position='sticky' top={0} zIndex={5} bg='transparent' mb={2}>
        <Box display='flex' flexDirection='column' width='100%' gap={0}>
          <Header search={search} setSearch={setSearch} />
          <HeaderMenu
            configs={visibleConfigs}
            selectedConfigs={selectedConfigs}
            setSelectedConfigs={setSelectedConfigs}
            initiatePortForwarding={initiatePortForwarding}
            startSelectedPortForwarding={startSelectedPortForwarding}
            stopSelectedPortForwarding={stopSelectedPortForwarding}
            stopAllPortForwarding={stopAllPortForwarding}
            abortStartOperation={abortStartOperation}
            abortStopOperation={abortStopOperation}
            isInitiating={isInitiating}
            isStopping={isStopping}
            toggleExpandAll={toggleExpandAll}
            expandedIndices={expandedIndices}
            groupCount={groups.length}
            view={view}
            facets={facets}
            setView={setView}
          />
        </Box>
      </Box>

      <Box
        className='table-container'
        css={{
          flex: 1,
          overflowY: 'auto',
          backgroundColor: 'app.panel',
          borderRadius: 'var(--border-radius)',
          padding: '4px',
          border: '1px solid',
          borderColor: 'app.border',
        }}
      >
        {groups.length === 0 && configs.length > 0 && (
          <Flex
            direction='column'
            align='center'
            justify='center'
            gap={2}
            height='100%'
            minHeight='120px'
          >
            <Text fontSize='xs' color='whiteAlpha.600'>
              No configs match your search or filters
            </Text>
            <Button
              size='xs'
              variant='ghost'
              height='24px'
              px={2}
              fontSize='11px'
              bg='whiteAlpha.50'
              border='1px solid'
              borderColor='app.border'
              _hover={{ bg: 'whiteAlpha.100' }}
              onClick={() => {
                setSearch('')
                if (view?.filters.length) {
                  setView({ ...view, filters: [] })
                }
              }}
            >
              Clear search and filters
            </Button>
          </Flex>
        )}
        <AccordionRoot
          className='accordion-root'
          multiple
          value={expandedIndices}
          onValueChange={handleAccordionChange}
        >
          {groups.map(group => (
            <GroupAccordion
              key={group.id}
              group={group}
              selectedIds={selectedIds}
              activePods={groupPods.get(group.id) ?? NO_PODS}
              deleteConfigs={deleteConfigs}
              handleEditConfig={handleEditConfig}
              handleDuplicateConfig={handleDuplicateConfig}
              handleSelectionChange={handleSelectionChange}
              handleCheckboxChange={handleCheckboxChange}
              pendingConfigActions={pendingConfigActions}
              toggleConfigForward={toggleConfigForward}
            />
          ))}
        </AccordionRoot>
      </Box>
    </Box>
  )
}
