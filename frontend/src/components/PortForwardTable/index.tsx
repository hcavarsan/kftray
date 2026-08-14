import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react'

import { Box } from '@chakra-ui/react'

import ConfigTabs from '@/components/ConfigTabs'
import Header from '@/components/Header'
import HeaderMenu from '@/components/HeaderMenu'
import ContextsAccordion from '@/components/PortForwardTable/ContextsAccordion'
import {
  getConfigGroup,
  useConfigsByGroup,
} from '@/components/PortForwardTable/useConfigsByGroup'
import { AccordionRoot, ValueChangeDetails } from '@/components/ui/accordion'
import { Config, TableProps } from '@/types'

const PortForwardTable: React.FC<TableProps> = ({
  configs,
  isInitiating,
  setIsInitiating,
  isStopping,
  initiatePortForwarding,
  startSelectedPortForwarding,
  stopSelectedPortForwarding,
  stopAllPortForwarding,
  abortStartOperation,
  abortStopOperation,
  handleEditConfig,
  handleDuplicateConfig,
  handleDeleteConfig,
  confirmDeleteConfig,
  isAlertOpen,
  setIsAlertOpen,
  selectedConfigs,
  setSelectedConfigs,
  openSettingsModal,
  openServerResourcesModal,
  tabs,
  activeTab,
  onSelectTab,
  onCreateTab,
  onRenameTab,
  onDeleteTab,
  tabHasConfigs,
}) => {
  const [search, setSearch] = useState<string>('')
  const [expandedIndices, setExpandedIndices] = useState<string[]>([])
  const prevSelectedConfigsRef = useRef<Config[]>(selectedConfigs)
  const [isSelectAllChecked, setIsSelectAllChecked] = useState<boolean>(false)
  const [selectedConfigsByGroup, setSelectedConfigsByGroup] = useState<
    Record<string, boolean>
  >({})
  const [isCheckboxAction, setIsCheckboxAction] = useState<boolean>(false)

  const filteredConfigs = useMemo(() => {
    const searchLower = search.toLowerCase()

    return configs
      .filter(
        config =>
          config.alias.toLowerCase().includes(searchLower) ||
          config.context.toLowerCase().includes(searchLower) ||
          getConfigGroup(config).toLowerCase().includes(searchLower) ||
          config.remote_address?.toLowerCase().includes(searchLower) ||
          config.local_port.toString().includes(searchLower),
      )
      .sort(
        (a, b) =>
          getConfigGroup(a).localeCompare(getConfigGroup(b)) ||
          a.alias.localeCompare(b.alias, undefined, { sensitivity: 'base' }),
      )
  }, [configs, search])

  const configsByGroup = useConfigsByGroup(filteredConfigs)

  useEffect(() => {
    const groupKeys = new Set(Object.keys(configsByGroup))

    setExpandedIndices(prev => {
      const next = prev.filter(key => groupKeys.has(key))

      return next.length === prev.length ? prev : next
    })
  }, [configsByGroup])

  useEffect(() => {
    if (prevSelectedConfigsRef.current !== selectedConfigs) {
      const newSelectedConfigsByGroup = Object.fromEntries(
        Object.entries(configsByGroup).map(([group, groupConfigs]) => [
          group,
          groupConfigs.every(config =>
            selectedConfigs.some(selected => selected.id === config.id),
          ),
        ]),
      )

      setSelectedConfigsByGroup(newSelectedConfigsByGroup)
      setIsSelectAllChecked(
        configs.every(config =>
          selectedConfigs.some(selected => selected.id === config.id),
        ),
      )
      prevSelectedConfigsRef.current = selectedConfigs
    }
  }, [selectedConfigs, configs, configsByGroup])

  useEffect(() => {
    setSelectedConfigs(prev =>
      prev.map(selected => configs.find(c => c.id === selected.id) || selected),
    )
  }, [configs, setSelectedConfigs])

  const toggleExpandAll = () => {
    const allGroups = Object.keys(configsByGroup)

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
    (group: string, isChecked: boolean) => {
      setIsCheckboxAction(true)
      const groupConfigs = filteredConfigs.filter(
        config => getConfigGroup(config) === group,
      )

      setSelectedConfigs(prev => {
        if (isChecked) {
          const newSelections = [...prev]

          groupConfigs.forEach(config => {
            if (!prev.some(p => p.id === config.id)) {
              newSelections.push(config)
            }
          })

          return newSelections
        }

        const configIdsFiltered = new Set(
          prev
            .filter(
              config =>
                getConfigGroup(config) !== group ||
                !filteredConfigs.some(fc => fc.id === config.id),
            )
            .map(config => config.id),
        )

        return prev.filter(config => configIdsFiltered.has(config.id))
      })
      setIsCheckboxAction(false)
    },
    [filteredConfigs, setSelectedConfigs],
  )

  const handleSelectionChange = useCallback(
    (config: Config, isSelected: boolean) => {
      const configGroup = getConfigGroup(config)
      const newSelection = isSelected
        ? [...selectedConfigs, config]
        : selectedConfigs.filter(c => c.id !== config.id)

      setSelectedConfigs(newSelection)

      const groupConfigs = configs.filter(c => getConfigGroup(c) === configGroup)
      const allGroupSelected = groupConfigs.every(groupConfig =>
        newSelection.some(selected => selected.id === groupConfig.id),
      )

      setSelectedConfigsByGroup(prev => ({
        ...prev,
        [configGroup]: allGroupSelected,
      }))
    },
    [configs, selectedConfigs, setSelectedConfigs],
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
            configs={search ? filteredConfigs : configs}
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
            configsByGroup={configsByGroup}
          />
          <ConfigTabs
            tabs={tabs}
            activeTab={activeTab}
            onSelectTab={onSelectTab}
            onCreateTab={onCreateTab}
            onRenameTab={onRenameTab}
            onDeleteTab={onDeleteTab}
            tabHasConfigs={tabHasConfigs}
          />
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
        <AccordionRoot
          className='accordion-root'
          multiple
          value={expandedIndices}
          onValueChange={handleAccordionChange}
        >
          {Object.entries(configsByGroup).map(([group, groupConfigs]) => (
            <ContextsAccordion
              key={group}
              group={group}
              groupConfigs={groupConfigs}
              selectedConfigs={selectedConfigs}
              handleDeleteConfig={handleDeleteConfig}
              confirmDeleteConfig={confirmDeleteConfig}
              handleEditConfig={handleEditConfig}
              handleDuplicateConfig={handleDuplicateConfig}
              isAlertOpen={isAlertOpen}
              setIsAlertOpen={setIsAlertOpen}
              handleSelectionChange={handleSelectionChange}
              selectedConfigsByGroup={selectedConfigsByGroup}
              handleCheckboxChange={handleCheckboxChange}
              isInitiating={isInitiating}
              setIsInitiating={setIsInitiating}
              isStopping={isStopping}
            />
          ))}
        </AccordionRoot>
      </Box>
    </Box>
  )
}

export default PortForwardTable
