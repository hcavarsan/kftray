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
  Text,
} from '@chakra-ui/react'

import PortForwardRow from '@/components/PortForwardTable/ContextsAccordion/PortForwardRow'
import {
  AccordionItem,
  AccordionItemContent,
  AccordionItemTrigger,
} from '@/components/ui/accordion'
import { Checkbox } from '@/components/ui/checkbox'
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
  const contextRunningCount = contextConfigs.filter(
    config => config.is_running,
  ).length
  const contextTotalCount = contextConfigs.length
  const contextProgressValue = (contextRunningCount / contextTotalCount) * 100

  return (
    <AccordionItem
      value={context}
      border='1px solid rgba(255, 255, 255, 0.06)'
      borderRadius='lg'
      marginBottom={2}
      mt={2}
      overflow='hidden'
    >
      <AccordionItemTrigger>
        <Box
          display='flex'
          justifyContent='space-between'
          alignItems='center'
          width='100%'
        >
          <Box flex='1' display='flex' alignItems='center' gap={3}>
            <Box onClick={e => e.stopPropagation()} mt={2}>
              <Checkbox
                size='sm'
                checked={
                  selectedConfigsByContext[context] ||
                  contextConfigs.every(config => config.is_running)
                }
                onCheckedChange={() =>
                  handleCheckboxChange(
                    context,
                    !selectedConfigsByContext[context],
                  )
                }
                disabled={contextConfigs.every(config => config.is_running)}
              />
            </Box>
            <Tag
              colorPalette='whiteAlpha'
              px={2}
              py={0.5}
              fontWeight='medium'
              fontSize='xs'
              borderRadius='sm'
              bg='rgba(255, 255, 255, 0.06)'
              border='1px solid rgba(255, 255, 255, 0.1)'
            >
              {context}
            </Tag>
          </Box>

          <Stack direction='row' align='center' gap={2}>
            <Tooltip
              content={`${contextRunningCount} running out of ${contextTotalCount} total`}
            >
              <Tag
                colorPalette='black'
                borderRadius='full'
                px={2}
                py={0.5}
                fontSize='xs'
                bg='rgba(0, 0, 0, 0.2)'
                border='1px solid rgba(255, 255, 255, 0.1)'
              >
                <Box display='flex' alignItems='center' gap={1}>
                  {contextRunningCount > 0 ? (
                    <RepeatIcon className='animate-spin' size={12} />
                  ) : (
                    <InfoIcon size={12} />
                  )}
                  <span>
                    {contextRunningCount}/{contextTotalCount}
                  </span>
                </Box>
              </Tag>
            </Tooltip>
            <ProgressRoot value={contextProgressValue} size='xs'>
              <ProgressBar
                css={{
                  width: '60px',
                  height: '4px',
                  bg: 'rgba(255, 255, 255, 0.1)',
                  '& > div': {
                    bg:
                      contextProgressValue === 100
                        ? 'rgba(74, 222, 128, 0.8)'
                        : contextProgressValue > 0
                          ? 'rgba(59, 130, 246, 0.8)'
                          : 'rgba(255, 255, 255, 0.2)',
                  },
                }}
              />
            </ProgressRoot>
          </Stack>
        </Box>
      </AccordionItemTrigger>

      <AccordionItemContent>
        <Box
          width='100%'
          px={1}
          py={0.5}
          bg='rgba(22, 22, 22, 0.5)'
          borderBottomRadius='md'
        >
          <TableRoot
            size='sm'
            variant='outline'
            border='none'
            borderRadius='none'
            interactive
          >
            <TableHeader>
              <TableRow>
                <TableColumnHeader
                  width='40%'
                  borderBottom='1px solid rgba(255, 255, 255, 0.06)'
                >
                  <Text textStyle='xs'>Alias</Text>
                </TableColumnHeader>
                <TableColumnHeader
                  width='20%'
                  borderBottom='1px solid rgba(255, 255, 255, 0.06)'
                >
                  <Text textStyle='xs'>Port</Text>
                </TableColumnHeader>
                <TableColumnHeader
                  width='20%'
                  borderBottom='1px solid rgba(255, 255, 255, 0.06)'
                >
                  <Text textStyle='xs'>Status</Text>
                </TableColumnHeader>
                <TableColumnHeader
                  width='20%'
                  borderBottom='1px solid rgba(255, 255, 255, 0.06)'
                >
                  <Text textStyle='xs'>Actions</Text>
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
      </AccordionItemContent>
    </AccordionItem>
  )
}

export default ContextsAccordion
