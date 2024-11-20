import React from 'react'
import { InfoIcon, RepeatIcon } from 'lucide-react'

import {
  Box,
  Flex,
  TableBody,
  TableColumnHeader,
  TableHeader,
  TableRoot,
} from '@chakra-ui/react'

import PortForwardRow from '@/components/PortForwardTable/ContextsAccordion/PortForwardRow'
import {
  AccordionItem,
  AccordionItemContent,
  AccordionItemTrigger,
} from '@/components/ui/accordion'
import { Checkbox } from '@/components/ui/checkbox'
import { ProgressBar, ProgressRoot } from '@/components/ui/progress'
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
  const columns = [
    { width: '40%', label: 'Alias' },
    { width: '20%', label: 'Port' },
    { width: '20%', label: 'Status' },
    { width: '20%', label: 'Actions' },
  ]

  return (
    <AccordionItem value={context} className='accordion-item'>
      <AccordionItemTrigger className='accordion-trigger'>
        <div className='accordion-header'>
          <div className='checkbox-wrapper'>
            <Box onClick={e => e.stopPropagation()}>
              <Checkbox
                size='xs'
                checked={selectedConfigsByContext[context]}
                onCheckedChange={() =>
                  handleCheckboxChange(
                    context,
                    !selectedConfigsByContext[context],
                  )
                }
                className='checkbox'
              />
            </Box>
            <span className='context-tag'>{context}</span>
          </div>

          <Flex align='center' gap={2}>
            <Tooltip
              content={`${contextRunningCount} running out of ${contextTotalCount} total`}
            >
              <span className='status-tag'>
                {contextRunningCount > 0 ? (
                  <RepeatIcon className='status-icon animate-spin' />
                ) : (
                  <InfoIcon className='status-icon' />
                )}
                <span>
                  {contextRunningCount}/{contextTotalCount}
                </span>
              </span>
            </Tooltip>
            <ProgressRoot
              value={contextProgressValue}
              css={{
                width: '40px',
                height: '3px',
                backgroundColor: 'rgba(255, 255, 255, 0.1)',
                borderRadius: '2px',
                overflow: 'hidden',
              }}
            >
              <ProgressBar
                css={{
                  height: '100%',
                  width: `${contextProgressValue}%`,
                  transition: 'all 0.2s ease-in-out',
                  backgroundColor:
                    contextProgressValue === 100
                      ? 'rgb(59, 130, 246)'
                      : contextProgressValue > 0
                        ? 'rgba(59, 130, 246, 0.8)'
                        : 'rgba(255, 255, 255, 0.2)',
                }}
              />
            </ProgressRoot>
          </Flex>
        </div>
      </AccordionItemTrigger>
      <AccordionItemContent>
        <Box
          width='100%'
          px={1}
          py={0.5}
          bg='rgba(22, 22, 22, 0.5)'
          borderRadius='md'
          border='none'
        >
          <TableRoot
            size='sm'
            variant='outline'
            border='none'
            borderRadius='md'
            interactive
            className='table-root'
          >
            <TableHeader>
              <tr>
                {columns.map(column => (
                  <TableColumnHeader
                    key={column.label}
                    className='table-header-cell'
                    style={{ width: column.width }}
                  >
                    {column.label}
                  </TableColumnHeader>
                ))}
              </tr>
            </TableHeader>
            <TableBody border='none'>
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
