import { memo, useEffect } from 'react'
import type { RowComponentProps } from 'react-window'
import { List, useDynamicRowHeight, useListCallbackRef } from 'react-window'

import { Box, Text } from '@chakra-ui/react'

import { ROW_HEIGHT_COLLAPSED } from './constants'
import { LogRow } from './LogRow'
import type { LogEntry, LogViewerListProps } from './types'

interface RowProps {
  entries: LogEntry[]
  expandedIds: Set<number>
  onToggleExpand: (id: number) => void
  searchText: string
}

/**
 * Only positions the row. The list re-renders every visible row whenever
 * something above it changes height, so the expensive content lives in the
 * memoized `LogRow`, which ignores position changes.
 */
function Row({
  index,
  style,
  ariaAttributes,
  entries,
  expandedIds,
  onToggleExpand,
  searchText,
}: RowComponentProps<RowProps>) {
  const entry = entries[index]

  return (
    <div style={style} {...ariaAttributes}>
      <LogRow
        entry={entry}
        isExpanded={expandedIds.has(entry.id)}
        onToggle={onToggleExpand}
        searchText={searchText}
      />
    </div>
  )
}

const rowKey = (index: number, { entries }: RowProps) => entries[index].id

function LogViewerListComponent({
  entries,
  expandedIds,
  onToggleExpand,
  searchText,
  autoFollow,
}: LogViewerListProps) {
  const [listRef, setListRef] = useListCallbackRef(null)
  const lastId = entries.at(-1)?.id

  // Measured heights are cached by row index, so they are dropped whenever
  // the rows behind those indices change (new lines, filters).
  const rowHeight = useDynamicRowHeight({
    defaultRowHeight: ROW_HEIGHT_COLLAPSED,
    key: `${entries.length}:${entries[0]?.id}:${lastId}`,
  })

  useEffect(() => {
    if (autoFollow && listRef && lastId !== undefined) {
      listRef.scrollToRow({ index: entries.length - 1, align: 'end' })
    }
  }, [autoFollow, listRef, lastId, entries.length])

  if (entries.length === 0) {
    return (
      <Box
        h='100%'
        display='flex'
        alignItems='center'
        justifyContent='center'
        flexDirection='column'
        gap={2}
      >
        <Text color='fg.faint' fontSize='13px'>
          No log entries to display
        </Text>
        <Text color='fg.faint' fontSize='11px'>
          Logs will appear here as they are generated
        </Text>
      </Box>
    )
  }

  return (
    <List
      listRef={setListRef}
      rowCount={entries.length}
      rowHeight={rowHeight}
      rowKey={rowKey}
      overscanCount={8}
      rowComponent={Row}
      rowProps={{ entries, expandedIds, onToggleExpand, searchText }}
      style={{ height: '100%', overflowX: 'hidden' }}
    />
  )
}

export const LogViewerList = memo(LogViewerListComponent)
