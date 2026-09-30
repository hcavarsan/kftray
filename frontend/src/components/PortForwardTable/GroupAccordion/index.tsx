import { memo, useMemo } from 'react'
import { InfoIcon, RepeatIcon } from 'lucide-react'

import {
  Box,
  Flex,
  TableBody,
  TableColumnHeader,
  TableHeader,
  TableRoot,
} from '@chakra-ui/react'

import { PortForwardRow } from '@/components/PortForwardTable/GroupAccordion/PortForwardRow'
import {
  AccordionItem,
  AccordionItemContent,
  AccordionItemTrigger,
} from '@/components/ui/accordion'
import { Checkbox } from '@/components/ui/checkbox'
import { ProgressBar, ProgressRoot } from '@/components/ui/progress'
import { Tooltip } from '@/components/ui/tooltip'
import type {
  Config,
  PendingConfigAction,
  PortForwardToggleAction,
  ResolvedGroup,
} from '@/types'

const columns = [
  { width: '40%', label: 'Alias' },
  { width: '20%', label: 'Port' },
  { width: '20%', label: 'Status' },
  { width: '20%', label: 'Actions' },
]

interface GroupAccordionProps {
  group: ResolvedGroup
  selectedIds: Set<number>
  activePods: Map<number, string | null>
  deleteConfigs: (ids: number[]) => Promise<boolean>
  handleEditConfig: (id: number) => Promise<void>
  handleDuplicateConfig: (id: number) => Promise<void>
  handleSelectionChange: (config: Config, isSelected: boolean) => void
  handleCheckboxChange: (group: ResolvedGroup, isChecked: boolean) => void
  pendingConfigActions: Map<number, PendingConfigAction>
  toggleConfigForward: (
    config: Config,
    action: PortForwardToggleAction,
  ) => Promise<void>
}

function GroupAccordionComponent({
  group,
  selectedIds,
  activePods,
  handleSelectionChange,
  handleCheckboxChange,
  pendingConfigActions,
  toggleConfigForward,
  deleteConfigs,
  handleEditConfig,
  handleDuplicateConfig,
}: GroupAccordionProps) {
  const groupConfigs = group.configs
  const isGroupSelected = useMemo(
    () => groupConfigs.every(config => selectedIds.has(config.id)),
    [groupConfigs, selectedIds],
  )

  const groupRunningCount = groupConfigs.filter(
    config => config.is_running,
  ).length
  const groupTotalCount = groupConfigs.length
  const groupProgressValue = (groupRunningCount / groupTotalCount) * 100

  return (
    <AccordionItem value={group.id} className='accordion-item'>
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
            <span className='context-tag'>{group.label}</span>
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
                backgroundColor: 'app.active',
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
                      ? 'blue.500'
                      : groupProgressValue > 0
                        ? 'app.accent'
                        : 'app.divider',
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
          bg='app.panel/50'
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
                  deleteConfigs={deleteConfigs}
                  handleEditConfig={handleEditConfig}
                  handleDuplicateConfig={handleDuplicateConfig}
                  selected={selectedIds.has(config.id)}
                  onSelectionChange={handleSelectionChange}
                  pendingAction={pendingConfigActions.get(config.id) ?? null}
                  activePod={activePods.get(config.id) ?? null}
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

export const GroupAccordion = memo(GroupAccordionComponent)
