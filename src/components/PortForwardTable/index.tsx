import React, { useMemo, useState } from 'react'
import {
  MdAdd,
  MdClose,
  MdFileDownload,
  MdFileUpload,
  MdMoreVert,
  MdRefresh,
} from 'react-icons/md'

import { SearchIcon } from '@chakra-ui/icons'
import {
  Accordion,
  AccordionButton,
  AccordionIcon,
  AccordionItem,
  AccordionPanel,
  Box,
  Button,
  ButtonGroup,
  Flex,
  IconButton,
  Input,
  InputGroup,
  InputLeftElement,
  Menu,
  MenuButton,
  MenuItem,
  MenuList,
  Table,
  Tbody,
  Text,
  Th,
  Thead,
  Tr,
  useColorModeValue,
} from '@chakra-ui/react'

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
}) => {
  const [search, setSearch] = useState('')
  const [expandedIndices, setExpandedIndices] = useState<number[]>([])

  const filteredConfigs = useMemo(() => {
    return search
      ? configs.filter(config =>
        config.service.toLowerCase().includes(search.toLowerCase()),
      )
      : configs
  }, [configs, search])

  const toggleExpandAll = () => {
    const allIndices = Object.keys(configsByContext).map((_, index) => index)

    if (expandedIndices.length === allIndices.length) {
      setExpandedIndices([])
    } else {
      setExpandedIndices(allIndices)
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

  // Calculate the count of configs and the count of configs running
  const configsCount = configs.length
  const runningConfigsCount = configs.filter(config => config.isRunning).length

  const configsByContext = groupByContext(configs)

  const bg = useColorModeValue('gray.50', 'gray.700')
  const accordionBg = useColorModeValue('gray.100', 'gray.800')
  const border = useColorModeValue('gray.200', 'gray.600')
  const borderColor = useColorModeValue('gray.200', 'gray.600')
  const textColor = useColorModeValue('gray.800', 'white')
  const boxShadow = useColorModeValue('base', 'md')
  const fontFamily = '\'Inter\', sans-serif'
  const accentColor = useColorModeValue('gray.100', 'gray.600') // use accent color for delineation

  const handleSearchChange = (event: React.ChangeEvent<HTMLInputElement>) => {
    setSearch(event.target.value)
  }
  const handleChange = (expandedIndex: number | number[]) => {
    setExpandedIndices(expandedIndex as number[])
  }

  return (
    <Flex
      direction='column'
      height='500px'
      maxHeight='500px'
      flex='1'
      width='100%'
      borderColor={borderColor}
    >
      <Flex justify='center' mb={5} mt={2}>
        <ButtonGroup variant='outline' spacing={2}>
          <Button
            leftIcon={<MdRefresh />}
            colorScheme='facebook'
            isLoading={isInitiating}
            loadingText='Starting...'
            onClick={startAllPortForwarding}
            isDisabled={
              isInitiating || !configs.some(config => !config.isRunning)
            }
            mr={1}
          >
            Start All
          </Button>
          <Button
            leftIcon={<MdClose />}
            colorScheme='facebook'
            isLoading={isStopping}
            loadingText='Stopping...'
            onClick={stopAllPortForwarding}
            isDisabled={isStopping || !configs.some(config => config.isRunning)}
          >
            Stop All
          </Button>
        </ButtonGroup>
      </Flex>

      <Flex justifyContent='space-between' mt='1' borderRadius='md'>
        <InputGroup size='xs' maxWidth='150px'>
          <InputLeftElement pointerEvents='none'>
            <SearchIcon color='gray.300' />
          </InputLeftElement>
          <Input
            type='text'
            placeholder='Search'
            onChange={handleSearchChange}
            borderRadius='md'
            size='xs'
            mr='2'
          />
        </InputGroup>
        <Flex mr='3'>
          <Button
            onClick={toggleExpandAll}
            size='xs'
            colorScheme='facebook'
            variant='outline'
            leftIcon={
              expandedIndices.length ===
              Object.keys(configsByContext).length ? (
                  <MdClose />
                ) : (
                  <MdAdd />
                )
            }
          >
            {expandedIndices.length === Object.keys(configsByContext).length
              ? 'Minimize All'
              : 'Expand All'}
          </Button>
          <Menu>
            <MenuButton
              as={IconButton}
              aria-label='Options'
              icon={<MdMoreVert />}
              size='xs'
              colorScheme='facebook'
              variant='outline'
              ml={2}
            />
            <MenuList>
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
        </Flex>
      </Flex>
      {search.trim() ? (
        <PortForwardSearchTable
          configs={filteredConfigs}
          handleEditConfig={handleEditConfig}
          handleDeleteConfig={handleDeleteConfig}
          confirmDeleteConfig={confirmDeleteConfig}
          updateConfigRunningState={updateConfigRunningState}
          isAlertOpen={isAlertOpen}
          setIsAlertOpen={setIsAlertOpen}
        />
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
            onChange={handleChange}
            borderColor={borderColor}
          >
            {Object.entries(configsByContext).map(
              ([context, contextConfigs]) => (
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
                      cluster: {context}
                    </Box>
                    <Box
                      flex='1'
                      textAlign='right'
                      fontSize='sm'
                      color={textColor}
                    >
                      ({contextConfigs.filter(c => c.isRunning).length}/
                      {contextConfigs.length})
                    </Box>
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
                            <Tr boxShadow={boxShadow} fontSize='10px'>
                              <Th
                                fontFamily={fontFamily}
                                fontSize='10px'
                                width='20%'
                              >
                                Service
                              </Th>
                              <Th
                                fontFamily={fontFamily}
                                fontSize='10px'
                                width='20%'
                              >
                                Namespace
                              </Th>
                              <Th
                                fontFamily={fontFamily}
                                fontSize='10px'
                                width='20%'
                              >
                                Port
                              </Th>
                              <Th
                                fontFamily={fontFamily}
                                fontSize='10px'
                                width='20%'
                              >
                                Status
                              </Th>
                              <Th
                                fontFamily={fontFamily}
                                fontSize='10px'
                                width='20%'
                              >
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
                                  setIsAlertOpen={setIsAlertOpen}
                                  updateConfigRunningState={
                                    updateConfigRunningState
                                  }
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
              ),
            )}
          </Accordion>
        </Flex>
      )}
    </Flex>
  )
}

export default PortForwardTable
