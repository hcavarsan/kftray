import React, { useEffect, useMemo, useRef, useState } from 'react'

import { Accordion, Flex, useColorModeValue } from '@chakra-ui/react'

import { Config, TableProps } from '../../types'
import Header from '../Header'
import HeaderMenu from '../HeaderMenu'

import ContextsAccordion from './ContextsAccordion'
import { useConfigsByContext } from './useConfigsByContext'

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
  const [expandedIndices, setExpandedIndices] = useState<number[]>([])
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
    const allIndices = Object.keys(configsByContext).map((_, index) => index)

    if (expandedIndices.length === allIndices.length) {
      setExpandedIndices([])
    } else {
      setExpandedIndices(allIndices)
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
  const boxShadow = useColorModeValue('base', 'lg')

  const handleAccordionChange = (expandedIndex: number | number[]) => {
    if (!isCheckboxAction) {
      setExpandedIndices(expandedIndex as number[])
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
    <Flex
      direction='column'
      height='100%'
      overflow='hidden'
      width='100%'
      borderColor={borderColor}
    >
      <Flex
        direction='row'
        alignItems='center'
        justifyContent='space-between'
        position='sticky'
        top='0'
        bg='gray.800'
        borderRadius='lg'
        width='98.4%'
        borderColor={borderColor}
      >
        <Header search={search} setSearch={setSearch} />
      </Flex>
      <Flex
        direction='row'
        alignItems='center'
        mt='2'
        justifyContent='space-between'
        position='relative'
        top='0'
        bg='gray.900'
        p='2'
        boxShadow={boxShadow}
        borderRadius='lg'
        border='1px'
        width='98.4%'
        borderColor={borderColor}
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
      </Flex>
      <Flex
        direction='column'
        maxHeight='100%'
        mt='5'
        overflowY='scroll'
        width='100%'
        borderBottom='none'
        borderRadius='lg'
        background='gray.1000'
        boxShadow='0 0 1px rgba(20, 20, 20, 0.50)'
        marginTop='1'
      >
        <Accordion
          allowMultiple
          index={expandedIndices}
          onChange={handleAccordionChange}
          borderColor={borderColor}
          width='full'
        >
          {Object.entries(configsByContext).map(
            ([context, contextConfigs], _contextIndex) => (
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
            ),
          )}
        </Accordion>
      </Flex>
    </Flex>
  )
}

export default PortForwardTable
