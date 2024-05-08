// components/PortForwardSearchTable/index.tsx
import React from 'react'

import {
  Box,
  Table,
  Tbody,
  Td,
  Th,
  Thead,
  Tr,
  useColorModeValue,
} from '@chakra-ui/react'

import { PortForwardSearchTableProps } from '../../types'
import PortForwardRow from '../PortForwardRow'

const PortForwardSearchTable: React.FC<PortForwardSearchTableProps> = ({
  configs,
  handleEditConfig,
  handleDeleteConfig,
  confirmDeleteConfig,
  updateConfigRunningState,
  isAlertOpen,
  setIsAlertOpen,
  isInitiating,
  isStopping,
  onSelectionChange,
  updateSelectionState,
  setIsInitiating,
}) => {
  const textColor = useColorModeValue('gray.400', 'gray.400')
  const boxShadow = useColorModeValue('base', 'md')
  const fontFamily = '\'Inter\', sans-serif'

  return (
    <Box overflowY='auto' width='100%'>
      <Table variant='simple' size='sm' style={{ tableLayout: 'fixed' }} mt='5'>
        <Thead>
          <Tr boxShadow={boxShadow} fontSize='10px'>
            <Th fontFamily={fontFamily} fontSize='10px' color={textColor}>
              Ctx
            </Th>
            <Th fontFamily={fontFamily} fontSize='10px' color={textColor}>
              Alias
            </Th>
            <Th fontFamily={fontFamily} fontSize='10px' color={textColor}>
              Port
            </Th>
            <Th fontFamily={fontFamily} fontSize='10px' color={textColor}>
              Status
            </Th>
            <Th fontFamily={fontFamily} fontSize='10px' color={textColor}>
              Action
            </Th>
          </Tr>
        </Thead>
        <Tbody>
          {configs.length > 0 ? (
            configs.map(config => (
              <PortForwardRow
                key={config.id}
                config={config}
                confirmDeleteConfig={confirmDeleteConfig}
                handleDeleteConfig={handleDeleteConfig}
                handleEditConfig={handleEditConfig}
                isAlertOpen={isAlertOpen}
                setIsAlertOpen={setIsAlertOpen}
                updateConfigRunningState={updateConfigRunningState}
                showContext={true}
                selected={false}
                isInitiating={isInitiating}
                setIsInitiating={setIsInitiating}
                isStopping={isStopping}
                onSelectionChange={isSelected =>
                  onSelectionChange(config, isSelected)
                }
                updateSelectionState={updateSelectionState}
              />
            ))
          ) : (
            <Tr>
              <Td colSpan={6} style={{ textAlign: 'center' }}>
                No Configurations Found
              </Td>
            </Tr>
          )}
        </Tbody>
      </Table>
    </Box>
  )
}

export default PortForwardSearchTable
