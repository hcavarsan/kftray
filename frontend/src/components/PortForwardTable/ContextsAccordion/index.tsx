import React from 'react'

import { InfoIcon, RepeatIcon } from '@chakra-ui/icons'
import {
  AccordionButton,
  AccordionIcon,
  AccordionItem,
  AccordionPanel,
  Box,
  Checkbox,
  Flex,
  Icon,
  Progress,
  Table,
  Tag,
  Tbody,
  Text,
  Th,
  Thead,
  Tooltip,
  Tr,
  useColorModeValue,
} from '@chakra-ui/react'

import { ContextsAccordionProps } from '../../../types'

import PortForwardRow from './PortForwardRow'

const ContextsAccordion: React.FC<ContextsAccordionProps> = ({
  context,
  contextConfigs,
  selectedConfigs,
  handleDeleteConfig,
  confirmDeleteConfig,
  handleEditConfig,
  isAlertOpen,
  setIsAlertOpen,
  updateConfigRunningState,
  handleSelectionChange,
  updateSelectionState,
  selectedConfigsByContext,
  handleCheckboxChange,
  isInitiating,
  setIsInitiating,
  isStopping,
}) => {
  const bg = useColorModeValue('gray.50', 'gray.700')
  const accordionBg = useColorModeValue('gray.100', 'gray.800')
  const borderColor = useColorModeValue('gray.200', 'gray.700')
  const textColor = useColorModeValue('gray.800', 'white')
  const boxShadow = useColorModeValue('base', 'lg')
  const fontFamily = '\'Inter\', sans-serif'

  const contextRunningCount = contextConfigs.filter(
    config => config.isRunning,
  ).length
  const contextTotalCount = contextConfigs.length
  const contextTagColorScheme = contextRunningCount > 0 ? 'facebook' : 'gray'
  const contextProgressValue = (contextRunningCount / contextTotalCount) * 100

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
        width='full'
        fontSize='10px'
      >
        <Box flex='1' textAlign='left' fontSize='sm' color={textColor}>
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
              onClick={event => event.stopPropagation()}
              isDisabled={contextConfigs.every(config => config.isRunning)}
            >
              <Tag
                size='sm'
                colorScheme='facebook'
                mr={2}
                p={1.5}
                fontWeight='semibold'
                fontSize='xs'
                borderRadius='lg'
                borderWidth='0px'
              >
                {context}
              </Tag>
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
              <Flex alignItems='center' justifyContent='center' width='100%'>
                {contextRunningCount > 0 ? (
                  <Icon as={RepeatIcon} />
                ) : (
                  <Icon as={InfoIcon} />
                )}
              </Flex>
            </Tag>
          </Tooltip>
          <Progress
            value={contextProgressValue}
            size='xs'
            colorScheme={contextTagColorScheme}
            borderRadius='lg'
            width='50px'
          />
        </Flex>

        <AccordionIcon color={textColor} />
      </AccordionButton>
      <AccordionPanel pb={4} borderColor={borderColor} fontFamily={fontFamily}>
        {contextConfigs.length > 0 ? (
          <Flex direction='column' width='100%' mt={0} p={0}>
            <Box>
              <Table
                variant='simple'
                size='sm'
                border='none'
                style={{ tableLayout: 'fixed' }}
              >
                <Thead>
                  <Tr boxShadow={boxShadow}>
                    <Th fontFamily={fontFamily} fontSize='10px' width='40%'>
                      Alias
                    </Th>
                    <Th fontFamily={fontFamily} fontSize='10px'>
                      Port
                    </Th>
                    <Th fontFamily={fontFamily} fontSize='10px'>
                      On/Off
                    </Th>
                    <Th fontFamily={fontFamily} fontSize='10px'>
                      Action
                    </Th>
                  </Tr>
                </Thead>
                <Tbody width='full'>
                  {contextConfigs.map(config => (
                    <PortForwardRow
                      key={config.id}
                      config={config}
                      handleDeleteConfig={handleDeleteConfig}
                      confirmDeleteConfig={confirmDeleteConfig}
                      handleEditConfig={handleEditConfig}
                      isAlertOpen={isAlertOpen}
                      selected={selectedConfigs.some(
                        selectedConfig => selectedConfig.id === config.id,
                      )}
                      onSelectionChange={isSelected =>
                        handleSelectionChange(config, isSelected)
                      }
                      updateSelectionState={updateSelectionState}
                      setIsAlertOpen={setIsAlertOpen}
                      updateConfigRunningState={updateConfigRunningState}
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
}

export default ContextsAccordion
