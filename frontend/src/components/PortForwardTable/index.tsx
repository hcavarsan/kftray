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
  stopAllPortForwarding,
  handleEditConfig,
  handleDeleteConfig,
  confirmDeleteConfig,
  isAlertOpen,
  setIsAlertOpen,
  selectedConfigs,
  setSelectedConfigs,
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
    setSelectedConfigs(prev =>
      prev.filter(selected => {
        const config = configs.find(c => c.id === selected.id)

        return config && !config.is_running
      }),
    )
  }, [configs, setSelectedConfigs])

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
      setIsSelectAllChecked(selectedConfigs.length === configs.length)
      prevSelectedConfigsRef.current = selectedConfigs
    }
  }, [selectedConfigs, configs, configsByContext])

  const toggleExpandAll = () => {
    const allContexts = Object.keys(configsByContext)

    setExpandedIndices(current =>
      current.length === allContexts.length ? [] : allContexts,
    )
  }

  const startSelectedPortForwarding = async () => {
    const configsToStart = selectedConfigs.filter(config => !config.is_running)

    if (configsToStart.length > 0) {
      await initiatePortForwarding(configsToStart)
    }
  }

  const handleAccordionChange = (details: ValueChangeDetails) => {
    if (!isCheckboxAction) {
      setExpandedIndices(details.value)
    }
  }

  const handleCheckboxChange = useCallback(
    (context: string, isChecked: boolean) => {
      setIsCheckboxAction(true)
      const selectableConfigs = filteredConfigs.filter(
        config => config.context === context && !config.is_running,
      )

      setSelectedConfigs(prev => {
        if (isChecked) {
          const newSelections = [...prev]

          selectableConfigs.forEach(config => {
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
      if (config.is_running) {
        return
      }

      setSelectedConfigs(prev => {
        const newSelection = isSelected
          ? [...prev, config]
          : prev.filter(c => c.id !== config.id)

        const contextConfigs = configs.filter(
          c => c.context === config.context && !c.is_running,
        )
        const allContextSelected = contextConfigs.every(contextConfig =>
          newSelection.some(selected => selected.id === contextConfig.id),
        )

        setSelectedConfigsByContext(prev => ({
          ...prev,
          [config.context]: allContextSelected,
        }))

        return newSelection
      })
    },
    [configs, setSelectedConfigs],
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
      <Box position='sticky' top={0} zIndex={10} bg='transparent' mb={2}>
        <Box display='flex' flexDirection='column' width='100%' gap={0}>
          <Header search={search} setSearch={setSearch} />
          <HeaderMenu
            isSelectAllChecked={isSelectAllChecked}
            setIsSelectAllChecked={setIsSelectAllChecked}
            configs={search ? filteredConfigs : configs}
            selectedConfigs={selectedConfigs}
            setSelectedConfigs={setSelectedConfigs}
            initiatePortForwarding={initiatePortForwarding}
            startSelectedPortForwarding={startSelectedPortForwarding}
            stopAllPortForwarding={stopAllPortForwarding}
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
