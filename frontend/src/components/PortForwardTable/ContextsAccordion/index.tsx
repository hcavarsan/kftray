import React from 'react'
import { InfoIcon, RepeatIcon } from 'lucide-react'

import {
  Box,
  Stack,
  TableBody,
  TableColumnHeader,
  TableHeader,
  TableRoot,
  TableRow,
} from '@chakra-ui/react'

import PortForwardRow from '@/components/PortForwardTable/ContextsAccordion/PortForwardRow'
import {
  AccordionItem,
  AccordionItemContent,
  AccordionItemTrigger,
  AccordionRoot,
} from '@/components/ui/accordion'
import { Checkbox } from '@/components/ui/checkbox'
import { useColorModeValue } from '@/components/ui/color-mode'
import { ProgressBar, ProgressRoot } from '@/components/ui/progress'
import { Tag } from '@/components/ui/tag'
import { Tooltip } from '@/components/ui/tooltip'
import { ContextsAccordionProps } from '@/types'

const ContextsAccordion: React.FC<ContextsAccordionProps> = ({
  context,
  contextConfigs,
  selectedConfigs,
  handleDeleteConfig,
  confirmDeleteConfig,
  handleEditConfig,
  isAlertOpen,
  setIsAlertOpen,
  handleSelectionChange,
  selectedConfigsByContext,
  handleCheckboxChange,
  isInitiating,
  setIsInitiating,
  isStopping,
}) => {
  const bg = useColorModeValue('bg.subtle', 'bg.subtle')
  const accordionBg = useColorModeValue('bg.default', 'bg.default')
  const borderColor = useColorModeValue('border.default', 'border.default')
  const textColor = useColorModeValue('fg.default', 'fg.default')
  const fontFamily = '\'Inter\', sans-serif'

  const contextRunningCount = contextConfigs.filter(
    config => config.is_running,
  ).length
  const contextTotalCount = contextConfigs.length
  const contextProgressValue = (contextRunningCount / contextTotalCount) * 100

  return (
    <AccordionRoot collapsible defaultValue={[context]}>
      <AccordionItem value={context}>
        <AccordionItemTrigger
          bg={accordionBg}
          mt={1}
          borderRadius='lg'
          borderWidth='1px'
          borderColor={borderColor}
          _hover={{ bg }}
          display='flex'
          justifyContent='space-between'
          alignItems='center'
          width='full'
          px={4}
          py={2}
        >
          <Box flex='1' textAlign='left' fontSize='xs' color={textColor}>
            <Box onClick={event => event.stopPropagation()}>
              <Checkbox
                size='xs'
                checked={
                  selectedConfigsByContext[context] ||
                  contextConfigs.every(config => config.is_running)
                }
                onCheckedChange={() => {
                  handleCheckboxChange(
                    context,
                    !selectedConfigsByContext[context],
                  )
                }}
                disabled={contextConfigs.every(config => config.is_running)}
              >
                <Tag
                  colorPalette='whiteAlpha'
                  marginRight={2}
                  padding={2}
                  fontWeight='semibold'
                  fontSize='xs'
                  borderRadius='lg'
                >
                  {context}
                </Tag>
              </Checkbox>
            </Box>
          </Box>

          <Stack direction='row' align='center'>
            <Tooltip
              content={`${contextRunningCount} running out of ${contextTotalCount} total`}
            >
              <Tag colorPalette='black' borderRadius='full' marginRight={1}>
                <Box display='flex' alignItems='center' justifyContent='center'>
                  {contextRunningCount > 0 ? (
                    <RepeatIcon width="15px" height="15px" />
                  ) : (
                    <InfoIcon width="15px" height="15px" />
                  )}
                </Box>
              </Tag>
            </Tooltip>
            <ProgressRoot value={contextProgressValue} size='xs'>
              <ProgressBar
                colorPalette='black'

                width='50px'
              />
            </ProgressRoot>
          </Stack>
        </AccordionItemTrigger>

        <AccordionItemContent>
          {contextConfigs.length > 0 ? (
            <Box width='100%'>
              <TableRoot variant='outline' size='sm'>
                <TableHeader>
                  <TableRow>
                    <TableColumnHeader fontSize='xs' fontFamily={fontFamily}>
                      Alias
                    </TableColumnHeader>
                    <TableColumnHeader fontSize='xs' fontFamily={fontFamily}>
                      Port
                    </TableColumnHeader>
                    <TableColumnHeader fontSize='xs' fontFamily={fontFamily}>
                      On/Off
                    </TableColumnHeader>
                    <TableColumnHeader fontSize='xs' fontFamily={fontFamily}>
                      Action
                    </TableColumnHeader>
                  </TableRow>
                </TableHeader>
                <TableBody>
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
                      setIsAlertOpen={setIsAlertOpen}
                      isInitiating={isInitiating}
                      setIsInitiating={setIsInitiating}
                      isStopping={isStopping}
                    />
                  ))}
                </TableBody>
              </TableRoot>
            </Box>
          ) : (
            <Box display='flex' justifyContent='center' p={6}>
              <Box as='span'>No Configurations Found for {context}</Box>
            </Box>
          )}
        </AccordionItemContent>
      </AccordionItem>
    </AccordionRoot>
  )
}

export default ContextsAccordion
