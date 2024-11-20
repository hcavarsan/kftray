import React, { useEffect, useMemo, useRef, useState } from 'react'

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
      prev.filter(
        selected =>
          !configs.some(
            config => config.id === selected.id && config.is_running,
          ),
      ),
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

  const handleCheckboxChange = (context: string, isChecked: boolean) => {
    setIsCheckboxAction(true)
    handleContextSelectionChange(context, isChecked)
    setIsCheckboxAction(false)
  }

  const handleSelectionChange = (config: Config, isSelected: boolean) => {
    setSelectedConfigs(prev => {
      const isCurrentlySelected = prev.some(c => c.id === config.id)

      if (isSelected === isCurrentlySelected) {
        return prev
      }

      return isSelected
        ? [...prev, config]
        : prev.filter(c => c.id !== config.id)
    })
  }

  const handleContextSelectionChange = (
    context: string,
    isContextSelected: boolean,
  ) => {
    setIsCheckboxAction(true)
    setSelectedConfigs(current => {
      const contextConfigs = configs.filter(
        config => config.context === context,
      )

      return isContextSelected
        ? [
          ...current,
          ...contextConfigs.filter(
            config => !current.some(selected => selected.id === config.id),
          ),
        ]
        : current.filter(config => config.context !== context)
    })
  }

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
            configs={configs}
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
