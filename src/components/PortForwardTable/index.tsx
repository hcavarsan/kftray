import React, { useEffect, useMemo, useRef, useState } from 'react'

import { CheckCircleIcon, InfoIcon } from '@chakra-ui/icons'
import {
  Accordion,
  AccordionButton,
  AccordionIcon,
  AccordionItem,
  AccordionPanel,
  AlertDialog,
  AlertDialogBody,
  AlertDialogContent,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogOverlay,
  Box,
  Button,
  Checkbox,
  Flex,
  Progress,
  Table,
  Tag,
  TagLabel,
  TagLeftIcon,
  Tbody,
  Text,
  Th,
  Thead,
  Tooltip,
  Tr,
  useColorModeValue,
} from '@chakra-ui/react'

import { Status, TableProps } from '../../types'
import ControlPanel from '../ControlPanel'
import Header from '../Header'
import PortForwardRow from '../PortForwardRow'

import { useConfigsByContext } from './useConfigsByContext'

const PortForwardTable: React.FC<TableProps> = ({
  configs,
  isInitiating,
  setIsInitiating,
  isStopping,
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
  confirmDeleteConfigs,
  isBulkAlertOpen,
  setIsBulkAlertOpen,
}) => {
  const [search, setSearch] = useState('')
  const [expandedIndices, setExpandedIndices] = useState<number[]>([])
  const cancelRef = React.useRef<HTMLButtonElement>(null)
  const prevSelectedConfigsRef = useRef(selectedConfigs)
  const [isSelectAllChecked, setIsSelectAllChecked] = useState(false)

  const [selectedConfigsByContext, setSelectedConfigsByContext] = useState<
    Record<string, boolean>
  >({})
  const [isCheckboxAction, setIsCheckboxAction] = useState(false)

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
  const bg = useColorModeValue('gray.50', 'gray.700')
  const accordionBg = useColorModeValue('gray.100', 'gray.800')
  const borderColor = useColorModeValue('gray.200', 'gray.700')
  const textColor = useColorModeValue('gray.800', 'white')
  const boxShadow = useColorModeValue('base', 'lg')
  const fontFamily = '\'Inter\', sans-serif'

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
      height='550px'
      maxHeight='550px'
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
        <AlertDialog
          isOpen={isBulkAlertOpen}
          leastDestructiveRef={cancelRef}
          onClose={() => setIsBulkAlertOpen(false)}
        >
          <AlertDialogOverlay
            style={{ alignItems: 'flex-start', justifyContent: 'flex-start' }}
            bg='transparent'
          >
            <AlertDialogContent>
              <AlertDialogHeader fontSize='sm' fontWeight='bold'>
                Delete Config(s)
              </AlertDialogHeader>

              <AlertDialogBody>
                Are you sure you want to delete the selected config(s)? This
                action cannot be undone.
              </AlertDialogBody>

              <AlertDialogFooter>
                <Button onClick={() => setIsBulkAlertOpen(false)}>
                  Cancel
                </Button>
                <Button colorScheme='red' onClick={confirmDeleteConfigs} ml={3}>
                  Delete
                </Button>
              </AlertDialogFooter>
            </AlertDialogContent>
          </AlertDialogOverlay>
        </AlertDialog>
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
        <ControlPanel
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
        height='550px'
        maxHeight='550px'
        pb='90px'
        flex='1'
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
        >
          {Object.entries(configsByContext).map(
            ([context, contextConfigs], _contextIndex) => {
              const contextRunningCount = contextConfigs.filter(
                config => config.isRunning,
              ).length
              const contextTotalCount = contextConfigs.length
              const contextTagColorScheme =
                contextRunningCount > 0 ? 'facebook' : 'gray'
              const contextProgressValue =
                (contextRunningCount / contextTotalCount) * 100

              return (
                <AccordionItem key={context} border='none'>
                  <AccordionButton
                    bg={accordionBg}
                    mt={2}
                    borderRadius='lg'
                    border='1px'
                    borderColor={borderColor}
                    boxShadow='lg'
                    _hover={{ bg: bg }}
                    _expanded={{ bg: accordionBg, boxShadow: 'lg' }}
                  >
                    <Box
                      flex='1'
                      textAlign='left'
                      fontSize='sm'
                      color={textColor}
                    >
                      <div onClick={event => event.stopPropagation()}>
                        <Checkbox
                          size='sm'
                          isChecked={
                            selectedConfigsByContext[context] ||
                            contextConfigs.every(config => config.isRunning)
                          }
                          onChange={event => {
                            event.stopPropagation()
                            handleCheckboxChange(
                              context,
                              !selectedConfigsByContext[context],
                            )
                          }}
                          onClick={event => {
                            event.stopPropagation()
                          }}
                          isDisabled={contextConfigs.every(
                            config => config.isRunning,
                          )}
                        >
                          cluster: {context}
                        </Checkbox>
                      </div>
                    </Box>
                    <Flex alignItems='center'>
                      <Tooltip
                        hasArrow
                        label={`${contextRunningCount} running out of ${contextTotalCount} total`}
                        bg='gray.300'
                        fontSize='xs'
                        lineHeight='tight'
                      >
                        <Tag
                          size='sm'
                          colorScheme={contextTagColorScheme}
                          borderRadius='full'
                          mr={2}
                        >
                          {contextRunningCount > 0 ? (
                            <TagLeftIcon as={CheckCircleIcon} />
                          ) : (
                            <TagLeftIcon as={InfoIcon} />
                          )}
                          <TagLabel>{`${contextRunningCount}/${contextTotalCount}`}</TagLabel>
                        </Tag>
                      </Tooltip>
                      <Progress
                        value={contextProgressValue}
                        size='xs'
                        colorScheme={contextTagColorScheme}
                        borderRadius='lg'
                        width='85px'
                      />
                    </Flex>
                    <AccordionIcon color={textColor} />
                  </AccordionButton>
                  <AccordionPanel
                    pb={4}
                    borderColor={borderColor}
                    fontFamily={fontFamily}
                  >
                    {contextConfigs.length > 0 ? (
                      <Flex direction='column' width='100%' mt={0} p={0}>
                        <Table
                          variant='simple'
                          size='sm'
                          border='none'
                          style={{ tableLayout: 'fixed' }}
                        >
                          <Thead
                            position='sticky'
                            top='0'
                            zIndex='sticky'
                            fontFamily={fontFamily}
                          >
                            <Tr boxShadow={boxShadow}>
                              <Th
                                fontFamily={fontFamily}
                                fontSize='10px'
                                width='40%'
                              >
                                Alias
                              </Th>
                              <Th fontFamily={fontFamily} fontSize='10px'>
                                Port
                              </Th>
                              <Th fontFamily={fontFamily} fontSize='10px'>
                                Status
                              </Th>
                              <Th fontFamily={fontFamily} fontSize='10px'>
                                Action
                              </Th>
                            </Tr>
                          </Thead>
                        </Table>
                        <Box>
                          <Table
                            size='sm'
                            border='none'
                            style={{ tableLayout: 'fixed' }}
                          >
                            <Tbody
                              width='full'
                              position='sticky'
                              top='0'
                              zIndex='sticky'
                            >
                              {contextConfigs.map(config => (
                                <PortForwardRow
                                  key={config.id}
                                  config={config}
                                  handleDeleteConfig={handleDeleteConfig}
                                  confirmDeleteConfig={confirmDeleteConfig}
                                  handleEditConfig={handleEditConfig}
                                  isAlertOpen={isAlertOpen}
                                  selected={selectedConfigs.some(
                                    selectedConfig =>
                                      selectedConfig.id === config.id,
                                  )}
                                  onSelectionChange={isSelected =>
                                    handleSelectionChange(config, isSelected)
                                  }
                                  updateSelectionState={updateSelectionState}
                                  setIsAlertOpen={setIsAlertOpen}
                                  updateConfigRunningState={
                                    updateConfigRunningState
                                  }
                                  isInitiating={isInitiating}
                                  setIsInitiating={setIsInitiating}
                                  isStopping={isStopping}
                                />
                              ))}
                            </Tbody>
                          </Table>
                        </Box>
                      </Flex>
                    ) : (
                      <Flex justify='center' p={6}>
                        <Text>No Configurations Found for {context}</Text>
                      </Flex>
                    )}
                  </AccordionPanel>
                </AccordionItem>
              )
            },
          )}
        </Accordion>
      </Flex>
    </Flex>
  )
}

export default PortForwardTable
