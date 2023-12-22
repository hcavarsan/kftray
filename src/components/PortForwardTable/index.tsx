import React from 'react'
import { MdClose, MdRefresh } from 'react-icons/md'

import {
  Box,
  Button,
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
  const startFilteredPortForwarding = () => {
    const stoppedConfigs = configs.filter(config => !config.isRunning)

    initiatePortForwarding(stoppedConfigs)
  }

  const stopFilteredPortForwarding = () => {
    const runningConfigs = configs.filter(config => config)

    stopPortForwarding(runningConfigs)
  }

  const hasRunningConfigs = configs.some(config => config.isRunning)
  const hasStoppedConfigs = configs.some(config => !config.isRunning)

  return (
    <>
      <Stack direction='row' spacing={4} justify='center' marginTop={0} mb={4}>
        <Button
          leftIcon={<MdRefresh />}
          colorScheme='facebook'
          isLoading={isInitiating}
          loadingText='Starting...'
          onClick={startFilteredPortForwarding}
          isDisabled={isInitiating || !hasStoppedConfigs}
        >
          Start Forward
        </Button>
        <Button
          leftIcon={<MdClose />}
          colorScheme='facebook'
          isLoading={isStopping}
          loadingText='Stopping...'
          onClick={stopFilteredPortForwarding}
          isDisabled={isStopping || !hasRunningConfigs}
        >
          Stop Forward
        </Button>
      </Stack>
      <Box
        width='100%'
        height='100%'
        overflowX='hidden'
        overflowY='auto'
        borderRadius='10px'
      >
        <Table variant='simple' size='sm'>
          <Thead>
            <Tr>
              <Th>Service</Th>
              <Th>Context</Th>
              <Th>Namespace</Th>
              <Th>Local Port</Th>
              <Th>Status</Th>
              <Th>Action</Th>
            </Tr>
          </Thead>
          <Tbody>
            {configs.length > 0 ? (
              configs.map(config => (
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
                <Td colSpan={6} textAlign='center'>
                  No Configurations Found
                </Td>
              </Tr>
            )}
          </Tbody>
        </Table>
      </Box>
    </>
  )
}

export default PortForwardTable
