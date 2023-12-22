// components/PortForwardTable/index.tsx
import React from 'react'
import { MdClose, MdRefresh } from 'react-icons/md'

import {
  Box,
  Button,
  Stack,
  Table,
  Tbody,
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
  isPortForwarding,
  initiatePortForwarding,
  stopPortForwarding,
  confirmDeleteConfig,
  handleEditConfig,
  handleDeleteConfig,
  isAlertOpen,
  setIsAlertOpen,
}) => {
  return (
    <>
      <Stack direction='row' spacing={4} justify='center' marginTop={0} mb={4}>
        <Button
          leftIcon={<MdRefresh />}
          colorScheme='facebook'
          isLoading={isInitiating}
          loadingText='Starting...'
          onClick={initiatePortForwarding}
          isDisabled={isPortForwarding}
        >
          Start Forward
        </Button>
        <Button
          leftIcon={<MdClose />}
          colorScheme='facebook'
          isLoading={isStopping}
          loadingText='Stopping...'
          onClick={stopPortForwarding} // Ensure this is correctly referencing the stopPortForwarding function
          isDisabled={!isPortForwarding}
        >
          Stop Forward
        </Button>
      </Stack>
      <Box width='100%' mt={0} p={0} borderRadius='10px'>
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
        </Table>
      </Box>
      <Box width='100%' height='100%' overflowY='auto' borderRadius='10px'>
        <Table variant='simple' size='sm' colorScheme='gray'>
          <Tbody>
            {configs.map(config => (
              <PortForwardRow
                key={config.id}
                config={config}
                confirmDeleteConfig={confirmDeleteConfig}
                handleDeleteConfig={handleDeleteConfig}
                handleEditConfig={handleEditConfig}
                isAlertOpen={isAlertOpen}
                setIsAlertOpen={setIsAlertOpen}
              />
            ))}
          </Tbody>
        </Table>
      </Box>
    </>
  )
}

export default PortForwardTable
