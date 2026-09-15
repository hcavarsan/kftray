import type React from 'react'
import { useEffect, useMemo, useRef } from 'react'
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
import type { ContextsAccordionProps } from '@/types'

const ContextsAccordion: React.FC<ContextsAccordionProps> = ({
  context,
  contextConfigs,
  selectedConfigs,
  handleSelectionChange,
  handleCheckboxChange,
  pendingConfigActions,
  toggleConfigForward,
  deleteConfigs,
  handleEditConfig,
  handleDuplicateConfig,
}) => {
  const isContextSelected = useMemo(() => {
    return contextConfigs.every(config =>
      selectedConfigs.some(selected => selected.id === config.id),
    )
  }, [contextConfigs, selectedConfigs])

  // `onSelectionChange` is handed to a memoized `PortForwardRow`; a fresh
  // arrow function per render would defeat that memoization for every row
  // even when nothing about the row changed. Cache one stable handler per
  // config id instead, reading the current config through a ref so the
  // cached function never closes over a stale value.
  const handleSelectionChangeRef = useRef(handleSelectionChange)

  useEffect(() => {
    handleSelectionChangeRef.current = handleSelectionChange
  }, [handleSelectionChange])

  const selectionHandlersRef = useRef(
    new Map<number, (isSelected: boolean) => void>(),
  )

  const contextConfigsRef = useRef(contextConfigs)

  contextConfigsRef.current = contextConfigs

  useEffect(() => {
    const currentIds = new Set(contextConfigs.map(config => config.id))

    for (const id of selectionHandlersRef.current.keys()) {
      if (!currentIds.has(id)) {
        selectionHandlersRef.current.delete(id)
      }
    }
  }, [contextConfigs])

  const getSelectionHandler = (configId: number) => {
    let handler = selectionHandlersRef.current.get(configId)

    if (!handler) {
      handler = isSelected => {
        const config = contextConfigsRef.current.find(
          item => item.id === configId,
        )

        if (config) {
          handleSelectionChangeRef.current(config, isSelected)
        }
      }
      selectionHandlersRef.current.set(configId, handler)
    }

    return handler
  }

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
                className='checkbox'
                size='xs'
                checked={isContextSelected}
                onCheckedChange={e =>
                  handleCheckboxChange(context, e.checked === true)
                }
                disabled={false}
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
                    className={`table-header-cell ${column.label === 'Alias' ? 'table-header-cell-alias' : ''}`}
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
                  deleteConfigs={deleteConfigs}
                  handleEditConfig={handleEditConfig}
                  handleDuplicateConfig={handleDuplicateConfig}
                  selected={selectedConfigs.some(
                    selectedConfig => selectedConfig.id === config.id,
                  )}
                  onSelectionChange={getSelectionHandler(config.id)}
                  pendingAction={pendingConfigActions.get(config.id) ?? null}
                  toggleConfigForward={toggleConfigForward}
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
