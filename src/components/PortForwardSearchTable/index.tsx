import React from 'react'

import { Box, Flex, Table, Tbody, Th, Thead, Tr } from '@chakra-ui/react'

import { Status } from '../../types'
import PortForwardRow from '../PortForwardRow'

interface PortForwardSearchTableProps {
  configs: Status[]
  handleEditConfig: (id: number) => void
  handleDeleteConfig: (id: number) => void
  confirmDeleteConfig: () => void
  updateConfigRunningState: (id: number, isRunning: boolean) => void
  isAlertOpen: boolean
  setIsAlertOpen: (isOpen: boolean) => void
}

const PortForwardSearchTable: React.FC<PortForwardSearchTableProps> = ({
  configs,
  handleEditConfig,
  handleDeleteConfig,
  confirmDeleteConfig,
  updateConfigRunningState,
  isAlertOpen,
  setIsAlertOpen,
}) => {
  return (
    <Flex
      direction='column'
      height='350px'
      maxHeight='350px'
      flex='1'
      overflowY='auto'
      width='100%'
    >
      <Table variant='simple'>
        <Thead>
          <Tr>
            <Th>Service</Th>
            <Th>Namespace</Th>
            <Th>Port</Th>
            <Th>Status</Th>
            <Th>Actions</Th>
          </Tr>
        </Thead>
        <Tbody>
          {configs.map(config => (
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
          ))}
        </Tbody>
      </Table>
    </Flex>
  )
}

export default PortForwardSearchTable
