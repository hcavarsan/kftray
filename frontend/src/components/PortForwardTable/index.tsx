import React, { useEffect, useMemo, useRef, useState } from 'react'

import { Accordion, Flex, useColorModeValue } from '@chakra-ui/react'

import useConfigStore from '../../store'
import { Status, TableProps } from '../../types'
import Header from '../Header'
import HeaderMenu from '../HeaderMenu'

import ContextsAccordion from './ContextsAccordion'
import { useConfigsByContext } from './useConfigsByContext'

const PortForwardTable: React.FC<TableProps> = ({
  initiatePortForwarding,
  stopPortForwarding,
  handleEditConfig,
  handleDeleteConfig,
  confirmDeleteConfig,
  isAlertOpen,
  setIsAlertOpen,
  updateConfigRunningState,
  selectedConfigs,
  setSelectedConfigs,
}) => {
  const [search, setSearch] = useState('')
  const [expandedIndices, setExpandedIndices] = useState<number[]>([])
  const prevSelectedConfigsRef = useRef(selectedConfigs)
  const [isSelectAllChecked, setIsSelectAllChecked] = useState(false)
  const [isCheckboxAction, setIsCheckboxAction] = useState(false)

  const { configs, isInitiating, setIsInitiating, isStopping, configState } =
    useConfigStore(state => ({
      configs: state.configs,
      isInitiating: state.isInitiating,
      setIsInitiating: state.setIsInitiating,
      isStopping: state.isStopping,
      configState: state.configState,
    }))

  const [selectedConfigsByContext, setSelectedConfigsByContext] = useState<
    Record<string, boolean>
  >({})

  const updateSelectionState = (id: number, isRunning: boolean) => {
    if (isRunning) {
      setSelectedConfigs(prev => prev.filter(config => config.id !== id))
    }
  }

  useEffect(() => {
    const isConfigRunning = (selectedConfig: Status) =>
      configs.some(
        config => config.id === selectedConfig.id && config.isRunning,
      )

    setSelectedConfigs(prevSelectedConfigs =>
      prevSelectedConfigs.filter(
        selectedConfig => !isConfigRunning(selectedConfig),
      ),
    )
  }, [configs, setSelectedConfigs])

  const filteredConfigs = useMemo(() => {
    const filterConfigsBySearch = (config: Status) =>
      config.alias.toLowerCase().includes(search.toLowerCase()) ||
      config.context.toLowerCase().includes(search.toLowerCase()) ||
      config.remote_address?.toLowerCase().includes(search.toLowerCase()) ||
      config.local_port.toString().includes(search.toLowerCase())

    const searchFiltered = search
      ? configs.filter(filterConfigsBySearch)
      : configs

    const compareConfigs = (a: Status, b: Status) =>
      a.alias.localeCompare(b.alias) || a.context.localeCompare(b.context)

    const sortedByAliasAsc = [...searchFiltered].sort(compareConfigs)

    return sortedByAliasAsc
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
    const configsToStart = selectedConfigs.filter(config => !config.isRunning)

    if (configsToStart.length > 0) {
      await initiatePortForwarding(configsToStart)
    }
  }

  const stopAllPortForwarding = () => {
    const runningConfigs = configs.filter(config => config.isRunning)

    stopPortForwarding(runningConfigs)
  }

  const configsByContext = useConfigsByContext(filteredConfigs)
  const borderColor = useColorModeValue('gray.200', 'gray.700')
  const boxShadow = useColorModeValue('base', 'lg')

  const handleAccordionChange = (expandedIndex: number | number[]) => {
    if (!isCheckboxAction) {
      setExpandedIndices(expandedIndex as number[])
    }
  }

  // eslint-disable-next-line max-params
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

  const handleSelectionChange = (config: Status, isSelected: boolean) => {
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
    currentSelectedConfigs: Status[],
    contextConfigs: Status[],
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
    currentSelectedConfigs: Status[],
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
                updateConfigRunningState={updateConfigRunningState}
                handleSelectionChange={handleSelectionChange}
                updateSelectionState={updateSelectionState}
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
