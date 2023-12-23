import React, { useMemo, useState } from 'react'
import { MdAdd, MdClose, MdRefresh } from 'react-icons/md'

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
  Input,
  InputGroup,
  InputLeftElement,
  Table,
  Tbody,
  Td,
  Text,
  Th,
  Thead,
  Tr,
  useColorModeValue,
  VStack,
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

  const handleSearchChange = (event: React.ChangeEvent<HTMLInputElement>) => {
    setSearch(event.target.value)
  }
  const handleChange = (expandedIndex: number | number[]) => {
    setExpandedIndices(expandedIndex as number[])
  }

  return (
    <Flex
      direction='column'
      height='450px'
      maxHeight='450px'
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

      <Flex justifyContent='space-between' mt='1' borderRadius='md' mb='2'>
        <Button
          onClick={toggleExpandAll}
          size='xs'
          colorScheme='facebook'
          variant='outline'
          leftIcon={
            expandedIndices.length === Object.keys(configsByContext).length ? (
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
        <Box
          bg={accordionBg}
          borderRadius='md'
          boxShadow='base'
          width='20%'
          mr={2}
        >
          <InputGroup size='xs'>
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
        </Box>
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
          height='450px'
          maxHeight='450px'
          pb='90px'
          flex='1'
          overflowY='scroll'
          width='100%'
          borderBottom='2px solid'
          borderColor={borderColor}
        >
          <Accordion
            allowMultiple
            index={expandedIndices}
            onChange={handleChange}
            borderTop='2px solid'
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
                      <VStack spacing={2} align='center'>
                        <Table variant='simple' size='sm' border='none'>
                          <Thead fontFamily={fontFamily}>
                            <Tr boxShadow={boxShadow}>
                              <Th fontFamily={fontFamily}>Service</Th>
                              <Th fontFamily={fontFamily}>Namespace</Th>
                              <Th fontFamily={fontFamily}>Port</Th>
                              <Th fontFamily={fontFamily}>Status</Th>
                              <Th fontFamily={fontFamily}>Action</Th>
                            </Tr>
                          </Thead>
                        </Table>
                        <Box>
                          <Table
                            size='sm'
                            border='none'
                            style={{ tableLayout: 'fixed' }}
                          >
                            <Tbody>
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
                      </VStack>
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
