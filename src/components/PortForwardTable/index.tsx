import React from 'react'
import { MdClose, MdRefresh } from 'react-icons/md'

import {
  Accordion,
  AccordionButton,
  AccordionIcon,
  AccordionItem,
  AccordionPanel,
  Box,
  Button,
  Flex,
  Stack,
  Table,
  Tbody,
  Td,
  Th,
  Thead,
  Tr,
} from '@chakra-ui/react'

import { TableProps } from '../../types'
import PortForwardRow from '../PortForwardRow'

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
  const startAllPortForwarding = () => {
    const stoppedConfigs = configs.filter(config => !config.isRunning)


    initiatePortForwarding(stoppedConfigs)
  }

  const stopAllPortForwarding = () => {
    const runningConfigs = configs.filter(config => config.isRunning)


    stopPortForwarding(runningConfigs)
  }

  const groupByContext = configs =>
    configs.reduce((group, config) => {
      const { context } = config


      group[context] = [...(group[context] || []), config]

      return group
    }, {})

  const configsByContext = groupByContext(configs)

  return (
    <Flex
      direction='column'
      height='450px'
      maxHeight='450px'
      pb='90px'
      flex='1'
      overflowY='scroll' // Apply overflow to the Flex container
      width='100%'
      scrollbarGutter='stable both-edges'
    >
      <Stack direction='row' spacing={4} justify='center' marginBottom={4}>
        <Button
          leftIcon={<MdRefresh />}
          colorScheme='facebook'
          isLoading={isInitiating}
          loadingText='Starting...'
          onClick={startAllPortForwarding}
          isDisabled={
            isInitiating || !configs.some(config => !config.isRunning)
          }
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
      </Stack>
      <Box
        position='relative'
        flex='1'
        overflowY='auto'
        maxHeight='450px'
        mt='10px'
      >
        <Accordion allowMultiple reduceMotion>
          {Object.entries(configsByContext).map(([context, contextConfigs]) => (
            <AccordionItem key={context} border='none'>
              <AccordionButton
                position='relative'
                border='1px'
                borderColor='gray.700'
                borderRadius='md'
                p={2}
                boxShadow='0 1px 3px rgba(0, 0, 0, 0.1), 0 1px 2px rgba(0, 0, 0, 0.06)'
                _expanded={{
                  bg: 'gray.800',
                  borderColor: 'gray.700',
                }}
                _hover={{
                  bg: 'gray.600',
                }}
              >
                <Box flex='1' textAlign='left' fontSize='sm'>
                  cluster: {context}
                </Box>
                <AccordionIcon />
              </AccordionButton>
              <AccordionPanel pb={4}>
                <Table
                  variant='simple'
                  size='sm'
                  border='1px'
                  borderColor='gray.700'
                  borderRadius='md'
                >
                  <Thead>
                    <Tr>
                      <Th width='20%'>Service</Th>
                      <Th width='20%'>Namespace</Th>
                      <Th width='20%'>Local Port</Th>
                      <Th width='20%'>Status</Th>
                      <Th width='20%'>Action</Th>
                    </Tr>
                  </Thead>
                  <Tbody>
                    {contextConfigs.length > 0 ? (
                      contextConfigs.map(config => (
                        <PortForwardRow
                          key={config.id}
                          config={config}
                          handleDeleteConfig={handleDeleteConfig}
                          confirmDeleteConfig={confirmDeleteConfig}
                          handleEditConfig={handleEditConfig}
                          isAlertOpen={isAlertOpen}
                          setIsAlertOpen={setIsAlertOpen}
                          updateConfigRunningState={updateConfigRunningState}
                        />
                      ))
                    ) : (
                      <Tr>
                        <Td colSpan={5} textAlign='center'>
                          No Configurations Found for {context}
                        </Td>
                      </Tr>
                    )}
                  </Tbody>
                </Table>
              </AccordionPanel>
            </AccordionItem>
          ))}
        </Accordion>
      </Box>
    </Flex>
  )
}

export default PortForwardTable
