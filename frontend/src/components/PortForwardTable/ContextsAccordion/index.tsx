import React, { useMemo } from 'react'
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
  group,
  groupConfigs,
  selectedConfigs,
  handleSelectionChange,
  handleCheckboxChange,
  isInitiating,
  setIsInitiating,
  isStopping,
  handleDeleteConfig,
  confirmDeleteConfig,
  handleEditConfig,
  handleDuplicateConfig,
  isAlertOpen,
  setIsAlertOpen,
}) => {
  const isGroupSelected = useMemo(() => {
    return groupConfigs.every(config =>
      selectedConfigs.some(selected => selected.id === config.id),
    )
  }, [groupConfigs, selectedConfigs])

  const groupRunningCount = groupConfigs.filter(
    config => config.is_running,
  ).length
  const groupTotalCount = groupConfigs.length
  const groupProgressValue = (groupRunningCount / groupTotalCount) * 100
  const columns = [
    { width: '40%', label: 'Alias' },
    { width: '20%', label: 'Port' },
    { width: '20%', label: 'Status' },
    { width: '20%', label: 'Actions' },
  ]

  return (
    <AccordionItem value={group} className='accordion-item'>
      <AccordionItemTrigger className='accordion-trigger'>
        <div className='accordion-header'>
          <div className='checkbox-wrapper'>
            <Box onClick={e => e.stopPropagation()}>
              <Checkbox
                className='checkbox'
                size='xs'
                checked={isGroupSelected}
                onCheckedChange={e =>
                  handleCheckboxChange(group, e.checked === true)
                }
                disabled={false}
              />
            </Box>
            <span className='group-tag'>{group}</span>
          </div>

          <Flex align='center' gap={2}>
            <Tooltip
              content={`${groupRunningCount} running out of ${groupTotalCount} total`}
            >
              <span className='status-tag'>
                {groupRunningCount > 0 ? (
                  <RepeatIcon className='status-icon animate-spin' />
                ) : (
                  <InfoIcon className='status-icon' />
                )}
                <span>
                  {groupRunningCount}/{groupTotalCount}
                </span>
              </span>
            </Tooltip>
            <ProgressRoot
              value={groupProgressValue}
              css={{
                width: '40px',
                height: '3px',
                backgroundColor: 'rgba(255, 255, 255, 0.1)',
                borderRadius: '2px',
              }}
            >
              <ProgressBar
                css={{
                  height: '100%',
                  width: `${groupProgressValue}%`,
                  transition: 'all 0.2s ease-in-out',
                  backgroundColor:
                    groupProgressValue === 100
                      ? 'rgb(59, 130, 246)'
                      : groupProgressValue > 0
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
                    className={`table-header-cell ${column.label === 'Alias' ? 'table-header-cell-alias' : ''}`}
                    style={{ width: column.width }}
                  >
                    {column.label}
                  </TableColumnHeader>
                ))}
              </tr>
            </TableHeader>
            <TableBody border='none'>
              {groupConfigs.map(config => (
                <PortForwardRow
                  key={config.id}
                  config={config}
                  handleDeleteConfig={handleDeleteConfig}
                  confirmDeleteConfig={confirmDeleteConfig}
                  handleEditConfig={handleEditConfig}
                  handleDuplicateConfig={handleDuplicateConfig}
                  isAlertOpen={isAlertOpen}
                  selected={selectedConfigs.some(
                    selectedConfig => selectedConfig.id === config.id,
                  )}
                  onSelectionChange={isSelected =>
                    handleSelectionChange(config, isSelected)
                  }
                  setIsAlertOpen={setIsAlertOpen}
                  _isInitiating={isInitiating}
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
