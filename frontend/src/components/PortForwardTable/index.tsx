import type React from 'react'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'

import { Box, Button, Flex, Text } from '@chakra-ui/react'

import Header from '@/components/Header'
import HeaderMenu from '@/components/HeaderMenu'
import ActiveFilters from '@/components/PortForwardTable/ActiveFilters'
import GroupAccordion from '@/components/PortForwardTable/GroupAccordion'
import {
  ALL_GROUP_ID,
  useConfigView,
} from '@/components/PortForwardTable/useConfigView'
import {
  AccordionRoot,
  type ValueChangeDetails,
} from '@/components/ui/accordion'
import type { Config, ResolvedGroup, TableProps } from '@/types'

const PortForwardTable: React.FC<TableProps> = ({
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
  openSettingsModal,
  openServerResourcesModal,
}) => {
  const [search, setSearch] = useState<string>('')
  const [expandedIndices, setExpandedIndices] = useState<string[]>([])
  const prevSelectedConfigsRef = useRef<Config[]>(selectedConfigs)
  const [isSelectAllChecked, setIsSelectAllChecked] = useState<boolean>(false)
  const [isCheckboxAction, setIsCheckboxAction] = useState<boolean>(false)

  const filteredConfigs = useMemo(() => {
    const searchLower = search.toLowerCase()

    return configs
      .filter(
        config =>
          config.alias.toLowerCase().includes(searchLower) ||
          config.context.toLowerCase().includes(searchLower) ||
          config.remote_address?.toLowerCase().includes(searchLower) ||
          config.local_port.toString().includes(searchLower) ||
          Object.entries(config.tags ?? {}).some(([key, value]) =>
            `${key}=${value}`.toLowerCase().includes(searchLower),
          ),
      )
      .sort(
        (a, b) =>
          a.alias.localeCompare(b.alias) || a.context.localeCompare(b.context),
      )
  }, [configs, search])

  const { view, facets, groups, setView } = useConfigView(
    configs,
    filteredConfigs,
  )
  const groupBy = view?.group_by
  const visibleConfigs = useMemo(
    () => groups.flatMap(group => group.configs),
    [groups],
  )

  useEffect(() => {
    setExpandedIndices(groupBy === null ? [ALL_GROUP_ID] : [])
  }, [groupBy])

  useEffect(() => {
    if (prevSelectedConfigsRef.current !== selectedConfigs) {
      setIsSelectAllChecked(
        configs.every(config =>
          selectedConfigs.some(selected => selected.id === config.id),
        ),
      )
      prevSelectedConfigsRef.current = selectedConfigs
    }
  }, [selectedConfigs, configs])

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
          const selectedIds = new Set(prev.map(config => config.id))

          return [
            ...prev,
            ...group.configs.filter(config => !selectedIds.has(config.id)),
          ]
        }

        return prev.filter(config => !groupIds.has(config.id))
      })
      setIsCheckboxAction(false)
    },
    [setSelectedConfigs],
  )

  const handleSelectionChange = useCallback(
    (id: number, isSelected: boolean) => {
      setSelectedConfigs(prev => {
        if (!isSelected) {
          return prev.filter(c => c.id !== id)
        }
        if (prev.some(c => c.id === id)) {
          return prev
        }
        const config = configs.find(c => c.id === id)

        return config ? [...prev, config] : prev
      })
    },
    [configs, setSelectedConfigs],
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
      {/* Header Section */}
      <Box position='sticky' top={0} zIndex={5} bg='transparent' mb={2}>
        <Box display='flex' flexDirection='column' width='100%' gap={0}>
          <Header
            search={search}
            setSearch={setSearch}
            openSettingsModal={openSettingsModal}
            openServerResourcesModal={openServerResourcesModal}
          />
          <HeaderMenu
            isSelectAllChecked={isSelectAllChecked}
            setIsSelectAllChecked={setIsSelectAllChecked}
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
          {view && <ActiveFilters view={view} setView={setView} />}
        </Box>
      </Box>

      {/* Content Section */}
      <Box
        className='table-container'
        css={{
          flex: 1,
          overflowY: 'auto',
          backgroundColor: '#161616',
          borderRadius: 'var(--border-radius)',
          padding: '4px',
          border: '1px solid rgba(255, 255, 255, 0.08)',
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
              border='1px solid rgba(255, 255, 255, 0.08)'
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
              selectedConfigs={selectedConfigs}
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

export default PortForwardTable
