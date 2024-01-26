import React, { useEffect, useMemo, useState } from 'react'
import {
  MdAdd,
  MdClose,
  MdFileDownload,
  MdFileUpload,
  MdMoreVert,
  MdRefresh,
} from 'react-icons/md'

import {
  CheckCircleIcon,
  ChevronDownIcon,
  ChevronUpIcon,
  SearchIcon,
} from '@chakra-ui/icons'
import {
  Accordion,
  AccordionButton,
  AccordionIcon,
  AccordionItem,
  AccordionPanel,
  Box,
  Button,
  ButtonGroup,
  Checkbox,
  Flex,
  Input,
  InputGroup,
  InputLeftElement,
  Menu,
  MenuButton,
  MenuItem,
  MenuList,
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
import { app } from '@tauri-apps/api'

import { Status, TableProps } from '../../types'
import PortForwardRow from '../PortForwardRow'
import PortForwardSearchTable from '../PortForwardSearchTable'

const PortForwardTable: React.FC<TableProps> = ({
  configs,
  isInitiating,
  isStopping,
  initiatePortForwarding,
  stopPortForwarding,
  handleEditConfig,
  handleDeleteConfig,
  confirmDeleteConfig,
  isAlertOpen,
  setIsAlertOpen,
  updateConfigRunningState,
  openModal,
  handleExportConfigs,
  handleImportConfigs,
  selectedConfigs,
  setSelectedConfigs,
}) => {
  const [search, setSearch] = useState('')
  const [expandedIndices, setExpandedIndices] = useState<number[]>([])
  const [version, setVersion] = useState('')
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
    app.getVersion().then(setVersion)
  }, [])

  useEffect(() => {
    setSelectedConfigs(prevSelectedConfigs =>
      prevSelectedConfigs.filter(
        selectedConfig =>
          !configs.some(
            config => config.id === selectedConfig.id && config.isRunning,
          ),
      ),
    )
  }, [configs, setSelectedConfigs])

  const filteredConfigs = useMemo(() => {
    const searchFiltered = search
      ? configs.filter(
        config =>
          config.alias.toLowerCase().includes(search.toLowerCase()) ||
            config.context.toLowerCase().includes(search.toLowerCase()) ||
            config.remote_address
            .toLowerCase()
            .includes(search.toLowerCase()) ||
            config.local_port.toString().includes(search.toLowerCase()),
      )
      : configs

    const sortedByAliasAsc = [...searchFiltered].sort(
      (a, b) =>
        a.alias.localeCompare(b.alias) || a.context.localeCompare(b.context),
    )

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

  const startAllPortForwarding = () => {
    const stoppedConfigs = configs.filter(config => !config.isRunning)

    initiatePortForwarding(stoppedConfigs)
  }

  const stopAllPortForwarding = () => {
    const runningConfigs = configs.filter(config => config.isRunning)

    stopPortForwarding(runningConfigs)
  }

  const groupByContext = (configs: Status[]) =>
    configs.reduce((group: Record<string, Status[]>, config: Status) => {
      const { context } = config

      group[context] = [...(group[context] || []), config]

      return group
    }, {})

  const configsByContext = groupByContext(configs)

  const bg = useColorModeValue('gray.50', 'gray.700')
  const accordionBg = useColorModeValue('gray.100', 'gray.800')
  const borderColor = useColorModeValue('gray.200', 'gray.600')
  const textColor = useColorModeValue('gray.800', 'white')
  const boxShadow = useColorModeValue('base', 'lg')
  const fontFamily = '\'Inter\', sans-serif'

  const handleSearchChange = (event: React.ChangeEvent<HTMLInputElement>) => {
    setSearch(event.target.value)
  }
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
    const newSelectedConfigsByContext: Record<string, boolean> = {}

    Object.entries(configsByContext).forEach(([context, contextConfigs]) => {
      newSelectedConfigsByContext[context] = contextConfigs.every(config =>
        selectedConfigs.some(selectedConfig => selectedConfig.id === config.id),
      )
    })

    setSelectedConfigsByContext(newSelectedConfigsByContext)
  }, [selectedConfigs, configsByContext])

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

      if (isContextSelected) {
        // eslint-disable-next-line max-len
        const newConfigsToAdd = contextConfigs.filter(
          config =>
            !currentSelectedConfigs.some(
              selectedConfig => selectedConfig.id === config.id,
            ),
        )

        return [...currentSelectedConfigs, ...newConfigsToAdd]
      } else {
        return currentSelectedConfigs.filter(
          config => config.context !== context,
        )
      }
    })

    setSelectedConfigsByContext(prev => ({
      ...prev,
      [context]: isContextSelected,
    }))
  }

  return (
    <Flex
      direction='column'
      height='500px'
      maxHeight='500px'
      overflow='hidden'
      width='100%'
      borderColor={borderColor}
    >
      <Flex
        direction='row'
        alignItems='left'
        bg={accordionBg}
        p='1'
        borderRadius='lg'
        width='98%'
        borderColor={borderColor}
        justifyContent='left'
      >
        <InputGroup size='xs' maxWidth='200px'>
          <InputLeftElement pointerEvents='none'>
            <SearchIcon color='gray.300' />
          </InputLeftElement>
          <Input
            type='text'
            placeholder='Search'
            onChange={handleSearchChange}
            borderRadius='md'
            size='xs'
          />
        </InputGroup>
      </Flex>
      <Flex
        direction='row'
        alignItems='center'
        mt='2'
        justifyContent='space-between'
        position='sticky'
        top='0'
        bg='gray.900'
        p='2'
        boxShadow={boxShadow}
        borderRadius='lg'
        border='1px'
        width='98.4%'
        borderColor={borderColor}
      >
        <Flex direction='row' justifyContent='center'>
          <ButtonGroup variant='outline'>
            <Button
              leftIcon={<MdRefresh />}
              colorScheme='facebook'
              isLoading={isInitiating}
              loadingText={isInitiating ? 'Starting...' : null}
              onClick={
                selectedConfigs.length > 0
                  ? startSelectedPortForwarding
                  : startAllPortForwarding
              }
              isDisabled={
                isInitiating ||
                (!selectedConfigs.length &&
                  !configs.some(config => !config.isRunning))
              }
              size='xs'
            >
              {selectedConfigs.length > 0 ? 'Start Selected' : 'Start All'}
            </Button>
            <Button
              leftIcon={<MdClose />}
              colorScheme='facebook'
              isLoading={isStopping}
              loadingText='Stopping...'
              onClick={stopAllPortForwarding}
              isDisabled={
                isStopping || !configs.some(config => config.isRunning)
              }
              size='xs'
            >
              Stop All
            </Button>
          </ButtonGroup>
        </Flex>
        <Flex justifyContent='flex-end'>
          <Button
            onClick={toggleExpandAll}
            size='xs'
            colorScheme='facebook'
            variant='outline'
            rightIcon={
              expandedIndices.length ===
              Object.keys(configsByContext).length ? (
                  <ChevronUpIcon />
                ) : (
                  <ChevronDownIcon />
                )
            }
          >
            {expandedIndices.length === Object.keys(configsByContext).length
              ? 'Collapse All'
              : 'Expand All'}
          </Button>
        </Flex>
      </Flex>
      {search.trim() ? (
        <Flex
          direction='column'
          height='500px'
          maxHeight='500px'
          pb='30px'
          flex='1'
          width='100%'
          mt='4'
          overflowY='scroll'
          borderBottom='none'
          borderRadius='lg'
          background='gray.1000'
          boxShadow='0 0 1px rgba(20, 20, 20, 0.50)'
          marginTop='1'
        >
          <PortForwardSearchTable
            configs={filteredConfigs}
            handleEditConfig={handleEditConfig}
            handleDeleteConfig={handleDeleteConfig}
            confirmDeleteConfig={confirmDeleteConfig}
            updateConfigRunningState={updateConfigRunningState}
            isAlertOpen={isAlertOpen}
            setIsAlertOpen={setIsAlertOpen}
            isInitiating={isInitiating}
            isStopping={isStopping}
          />
        </Flex>
      ) : (
        <Flex
          direction='column'
          height='500px'
          maxHeight='500px'
          pb='90px'
          flex='1'
          mt='4'
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
              ([context, contextConfigs], contextIndex) => {
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
                        >
                          <Tag
                            size='sm'
                            colorScheme={contextTagColorScheme}
                            borderRadius='full'
                            mr={2}
                          >
                            {contextRunningCount > 0 && (
                              <TagLeftIcon as={CheckCircleIcon} />
                            )}
                            <TagLabel>{`${contextRunningCount}/${contextTotalCount}`}</TagLabel>
                          </Tag>
                        </Tooltip>
                        <Progress
                          value={contextProgressValue}
                          size='xs'
                          colorScheme={contextTagColorScheme}
                          borderRadius='lg'
                          width='70px'
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
      )}
      <Flex
        justifyContent='space-between'
        align='center'
        mt='3'
        alignItems='center'
        maxWidth='100%'
      >
        <Menu placement='top'>
          <MenuButton
            as={Button}
            rightIcon={<MdMoreVert />}
            size='xs'
            colorScheme='facebook'
            variant='outline'
            borderRadius='md'
            width='85px'
          >
            Options
          </MenuButton>

          <MenuList zIndex='popover'>
            <MenuItem icon={<MdAdd />} onClick={openModal}>
              Add New Config
            </MenuItem>
            <MenuItem icon={<MdFileUpload />} onClick={handleExportConfigs}>
              Export Configs
            </MenuItem>
            <MenuItem icon={<MdFileDownload />} onClick={handleImportConfigs}>
              Import Configs
            </MenuItem>
          </MenuList>
        </Menu>
        <Text
          fontSize='sm'
          textAlign='center'
          color='gray.400'
          fontFamily='Inter, sans-serif'
          p={2}
          borderColor={useColorModeValue('gray.200', 'gray.700')}
        >
          {version}
        </Text>
      </Flex>
    </Flex>
  )
}

export default PortForwardTable
