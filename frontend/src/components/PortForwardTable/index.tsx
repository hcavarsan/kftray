import React, { useEffect, useMemo, useRef, useState } from 'react'

import { Box } from '@chakra-ui/react'

import Header from '@/components/Header'
import HeaderMenu from '@/components/HeaderMenu'
import ContextsAccordion from '@/components/PortForwardTable/ContextsAccordion'
import { useConfigsByContext } from '@/components/PortForwardTable/useConfigsByContext'
import { AccordionRoot } from '@/components/ui/accordion'
import { useColorModeValue } from '@/components/ui/color-mode'
import { Config, TableProps } from '@/types'

type ValueChangeDetails = {
  value: string[]
}

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

  useEffect(() => {
    const isConfigRunning = (selectedConfig: Config) =>
      configs.some(
        config => config.id === selectedConfig.id && config.is_running,
      )

    setSelectedConfigs(prevSelectedConfigs =>
      prevSelectedConfigs.filter(
        selectedConfig => !isConfigRunning(selectedConfig),
      ),
    )
  }, [configs, setSelectedConfigs])

  const filteredConfigs = useMemo(() => {
    const filterConfigsBySearch = (config: Config) =>
      config.alias.toLowerCase().includes(search.toLowerCase()) ||
      config.context.toLowerCase().includes(search.toLowerCase()) ||
      config.remote_address?.toLowerCase().includes(search.toLowerCase()) ||
      config.local_port.toString().includes(search.toLowerCase())

    const searchFiltered = search
      ? configs.filter(filterConfigsBySearch)
      : configs

    const compareConfigs = (a: Config, b: Config) =>
      a.alias.localeCompare(b.alias) || a.context.localeCompare(b.context)

    return [...searchFiltered].sort(compareConfigs)
  }, [configs, search])

  const toggleExpandAll = () => {
    const allContexts = Object.keys(configsByContext)

    if (expandedIndices.length === allContexts.length) {
      setExpandedIndices([])
    } else {
      setExpandedIndices(allContexts)
    }
  }

  const startSelectedPortForwarding = async () => {
    const configsToStart = selectedConfigs.filter(
      (config: Config) => !config.is_running,
    )

    if (configsToStart.length > 0) {
      await initiatePortForwarding(configsToStart)
    }
  }

  const configsByContext = useConfigsByContext(filteredConfigs)
  const borderColor = useColorModeValue('gray.200', 'gray.700')
  const boxShadow = useColorModeValue('base', 'sm')

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

  useEffect(() => {
    if (prevSelectedConfigsRef.current !== selectedConfigs) {
      const newSelectedConfigsByContext: Record<string, boolean> = {}

      for (const context of Object.keys(configsByContext)) {
        newSelectedConfigsByContext[context] = configsByContext[context].every(
          config =>
            selectedConfigs.some(
              selectedConfig => selectedConfig.id === config.id,
            ),
        )
      }

      setSelectedConfigsByContext(newSelectedConfigsByContext)
      setIsSelectAllChecked(selectedConfigs.length === configs.length)
      prevSelectedConfigsRef.current = selectedConfigs
    }
  }, [selectedConfigs, configs, configsByContext])

  const handleSelectionChange = (config: Config, isSelected: boolean) => {
    setSelectedConfigs(prevSelectedConfigs => {
      const isSelectedCurrently = prevSelectedConfigs.some(
        c => c.id === config.id,
      )

      if (isSelected && !isSelectedCurrently) {
        return [...prevSelectedConfigs, config]
      } else if (!isSelected && isSelectedCurrently) {
        return prevSelectedConfigs.filter(c => c.id !== config.id)
      } else {
        return prevSelectedConfigs
      }
    })
  }

  const handleContextSelectionChange = (
    context: string,
    isContextSelected: boolean,
  ) => {
    setIsCheckboxAction(true)
    setSelectedConfigs(currentSelectedConfigs => {
      const contextConfigs = configs.filter(
        config => config.context === context,
      )

      return isContextSelected
        ? addContextConfigs(currentSelectedConfigs, contextConfigs)
        : removeContextConfigs(currentSelectedConfigs, context)
    })
  }

  function addContextConfigs(
    currentSelectedConfigs: Config[],
    contextConfigs: Config[],
  ) {
    const newConfigsToAdd = contextConfigs.filter(
      config =>
        !currentSelectedConfigs.some(
          selectedConfig => selectedConfig.id === config.id,
        ),
    )

    return [...currentSelectedConfigs, ...newConfigsToAdd]
  }

  function removeContextConfigs(
    currentSelectedConfigs: Config[],
    context: string,
  ) {
    return currentSelectedConfigs.filter(config => config.context !== context)
  }

  return (
    <Box
      display="flex"
      flexDirection="column"
      height="90%"
      width="100%"
      overflow="hidden"
      bg="#111111"
      position="relative"
    >
      {/* Header Section */}
      <Box
        position="sticky"
        top={0}
        zIndex={10}
        bg="#111111"

        mb={2}
      >
        <Box
          display="flex"
          flexDirection="row"
          alignItems="center"
          justifyContent="space-between"
          width="100%"
        >
          <Header search={search} setSearch={setSearch} />
        </Box>
      </Box>

      {/* Menu Section */}
      <Box


        bg="#161616"
        borderRadius="lg"
        border="1px solid rgba(255, 255, 255, 0.08)"
        overflow="hidden"
      >
        <Box
          p={1}
        >
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

      {/* Main Content Section */}
      <Box
        flex={1}
        mt={2}

        overflowY="auto"
        bg="#161616"
        borderRadius="lg"
        border="1px solid rgba(255, 255, 255, 0.08)"
        css={{
          '&::-webkit-scrollbar': {
            width: '4px',
          },
          '&::-webkit-scrollbar-track': {
            background: 'transparent',
          },
          '&::-webkit-scrollbar-thumb': {
            background: 'rgba(255, 255, 255, 0.1)',
            borderRadius: '2px',
          },
          '&::-webkit-scrollbar-thumb:hover': {
            background: 'rgba(255, 255, 255, 0.2)',
          },
        }}
      >
        <AccordionRoot
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

      {/* Bottom Spacing */}
      <Box h={4} />
    </Box>
  )
}

export default PortForwardTable
