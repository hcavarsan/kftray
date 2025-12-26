import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react'

import { Box } from '@chakra-ui/react'

import Header from '@/components/Header'
import HeaderMenu from '@/components/HeaderMenu'
import ContextsAccordion from '@/components/PortForwardTable/ContextsAccordion'
import { useConfigsByContext } from '@/components/PortForwardTable/useConfigsByContext'
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
}) => {
  const [search, setSearch] = useState<string>('')
  const [expandedIndices, setExpandedIndices] = useState<string[]>([])
  const prevSelectedConfigsRef = useRef<Config[]>(selectedConfigs)
  const [isSelectAllChecked, setIsSelectAllChecked] = useState<boolean>(false)
  const [selectedConfigsByContext, setSelectedConfigsByContext] = useState<
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
          config.remote_address?.toLowerCase().includes(searchLower) ||
          config.local_port.toString().includes(searchLower),
      )
      .sort(
        (a, b) =>
          a.alias.localeCompare(b.alias) || a.context.localeCompare(b.context),
      )
  }, [configs, search])

  const configsByContext = useConfigsByContext(filteredConfigs)

  useEffect(() => {
    if (prevSelectedConfigsRef.current !== selectedConfigs) {
      const newSelectedConfigsByContext = Object.fromEntries(
        Object.entries(configsByContext).map(([context, contextConfigs]) => [
          context,
          contextConfigs.every(config =>
            selectedConfigs.some(selected => selected.id === config.id),
          ),
        ]),
      )

      setSelectedConfigsByContext(newSelectedConfigsByContext)
      setIsSelectAllChecked(
        configs.every(config =>
          selectedConfigs.some(selected => selected.id === config.id),
        ),
      )
      prevSelectedConfigsRef.current = selectedConfigs
    }
  }, [selectedConfigs, configs, configsByContext])

  useEffect(() => {
    setSelectedConfigs(prev =>
      prev.map(selected => configs.find(c => c.id === selected.id) || selected),
    )
  }, [configs, setSelectedConfigs])

  const toggleExpandAll = () => {
    const allContexts = Object.keys(configsByContext)

    setExpandedIndices(current =>
      current.length === allContexts.length ? [] : allContexts,
    )
  }

  const handleAccordionChange = (details: ValueChangeDetails) => {
    if (!isCheckboxAction) {
      setExpandedIndices(details.value)
    }
  }

  const handleCheckboxChange = useCallback(
    (context: string, isChecked: boolean) => {
      setIsCheckboxAction(true)
      const contextConfigs = filteredConfigs.filter(
        config => config.context === context,
      )

      setSelectedConfigs(prev => {
        if (isChecked) {
          const newSelections = [...prev]

          contextConfigs.forEach(config => {
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
                config.context !== context ||
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
      const newSelection = isSelected
        ? [...selectedConfigs, config]
        : selectedConfigs.filter(c => c.id !== config.id)

      setSelectedConfigs(newSelection)

      const contextConfigs = configs.filter(c => c.context === config.context)
      const allContextSelected = contextConfigs.every(contextConfig =>
        newSelection.some(selected => selected.id === contextConfig.id),
      )

      setSelectedConfigsByContext(prev => ({
        ...prev,
        [config.context]: allContextSelected,
      }))
    },
    [configs, selectedConfigs, setSelectedConfigs],
  )

  return (
    <Box
      display='flex'
      flexDirection='column'
      height='88%'
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
            configsByContext={configsByContext}
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
          {Object.entries(configsByContext).map(([context, contextConfigs]) => (
            <ContextsAccordion
              key={context}
              context={context}
              contextConfigs={contextConfigs}
              selectedConfigs={selectedConfigs}
              handleDeleteConfig={handleDeleteConfig}
              confirmDeleteConfig={confirmDeleteConfig}
              handleEditConfig={handleEditConfig}
              handleDuplicateConfig={handleDuplicateConfig}
              isAlertOpen={isAlertOpen}
              setIsAlertOpen={setIsAlertOpen}
              handleSelectionChange={handleSelectionChange}
              selectedConfigsByContext={selectedConfigsByContext}
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
