// components/PortForwardSearchTable/index.tsx
import React from 'react'

import {
  Box,
  IconButton,
  Switch,
  Table,
  Tbody,
  Td,
  Th,
  Thead,
  Tr,
  useColorModeValue,
} from '@chakra-ui/react'
import { faPen, faTrash } from '@fortawesome/free-solid-svg-icons'
import { FontAwesomeIcon } from '@fortawesome/react-fontawesome'

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
  const textColor = useColorModeValue('gray.100', 'gray.100')
  const boxShadow = useColorModeValue('base', 'md')
  const fontFamily = '\'Inter\', sans-serif'

  return (
    <Box overflowY='auto' width='100%'>
      <Table variant='simple' size='sm' style={{ tableLayout: 'fixed' }} mt='5'>
        <Thead>
          <Tr boxShadow={boxShadow} fontSize='10px'>
            <Th
              fontFamily={fontFamily}
              fontSize='10px'
              width='20%'
              color={textColor}
            >
              Context
            </Th>
            <Th
              fontFamily={fontFamily}
              fontSize='10px'
              width='20%'
              color={textColor}
            >
              Service
            </Th>
            <Th
              fontFamily={fontFamily}
              fontSize='10px'
              width='25%'
              color={textColor}
            >
              Namespace
            </Th>
            <Th
              fontFamily={fontFamily}
              fontSize='10px'
              width='20%'
              color={textColor}
            >
              Port
            </Th>
            <Th
              fontFamily={fontFamily}
              fontSize='10px'
              width='20%'
              color={textColor}
            >
              Status
            </Th>
            <Th
              fontFamily={fontFamily}
              fontSize='10px'
              width='20%'
              color={textColor}
            >
              Actions
            </Th>
          </Tr>
        </Thead>
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
              updateConfigRunningState={updateConfigRunningState}
              showContext={true}
            />
          ))}
        </Tbody>
      </Table>
      {configs.length === 0 && (
        <Box textAlign='center' py='5'>
          No configurations found.
        </Box>
      )}
    </Box>
  )
}

export default PortForwardSearchTable
