import {
  memo,
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from 'react'
import type { ListImperativeAPI, RowComponentProps } from 'react-window'
import { List, useDynamicRowHeight } from 'react-window'

import { Box, Text } from '@chakra-ui/react'

import { ROW_HEIGHT_COLLAPSED } from './constants'
import { LogRow } from './LogRow'
import type { LogViewerListProps } from './types'

interface RowProps {
  entries: LogViewerListProps['entries']
  expandedIds: Set<number>
  onToggleExpand: (id: number) => void
  searchText?: string
}

function LogViewerListComponent({
  entries,
  expandedIds,
  onToggleExpand,
  searchText,
  autoFollow = false,
}: LogViewerListProps) {
  const [listRef, setListRef] = useState<ListImperativeAPI | null>(null)
  const containerRef = useRef<HTMLDivElement>(null)
  const [dimensions, setDimensions] = useState({ width: 0, height: 0 })
  const prevEntriesLengthRef = useRef(entries.length)

  const dynamicRowHeight = useDynamicRowHeight({
    defaultRowHeight: ROW_HEIGHT_COLLAPSED,
    key: `${entries.length}-${expandedIds.size}`,
  })

  useLayoutEffect(() => {
    const updateDimensions = () => {
      if (containerRef.current) {
        const { clientWidth, clientHeight } = containerRef.current

        setDimensions(prev => {
          if (prev.width !== clientWidth || prev.height !== clientHeight) {
            return { width: clientWidth, height: clientHeight }
          }

          return prev
        })
      }
    }

    updateDimensions()

    const resizeObserver = new ResizeObserver(() => {
      requestAnimationFrame(updateDimensions)
    })

    if (containerRef.current) {
      resizeObserver.observe(containerRef.current)
    }

    return () => resizeObserver.disconnect()
  }, [])

  const handleHeightChange = useCallback(
    (id: number, height: number) => {
      const index = entries.findIndex(e => e.id === id)


      if (index !== -1) {
        dynamicRowHeight.setRowHeight(index, height)
      }
    },
    [entries, dynamicRowHeight],
  )

  useEffect(() => {
    if (autoFollow && listRef && entries.length > 0) {
      requestAnimationFrame(() => {
        listRef?.scrollToRow({ index: entries.length - 1, align: 'end' })
      })
    }
    prevEntriesLengthRef.current = entries.length
  }, [entries.length, autoFollow, listRef])

  useEffect(() => {
    if (autoFollow && listRef && entries.length > 0) {
      listRef.scrollToRow({ index: entries.length - 1, align: 'end' })
    }
  }, [autoFollow, entries.length, listRef])

  const Row = useCallback(
    ({
      index,
      style,
      entries: rowEntries,
      expandedIds: rowExpandedIds,
      onToggleExpand: rowOnToggleExpand,
      searchText: rowSearchText,
    }: RowComponentProps<RowProps>) => {
      const entry = rowEntries[index]

      if (!entry) {
        return null
      }

      const isExpanded = rowExpandedIds.has(entry.id)

      return (
        <LogRow
          entry={entry}
          isExpanded={isExpanded}
          onToggle={() => rowOnToggleExpand(entry.id)}
          onHeightChange={handleHeightChange}
          style={style}
          searchText={rowSearchText}
        />
      )
    },
    [handleHeightChange],
  )

  if (entries.length === 0) {
    return (
      <Box
        ref={containerRef}
        h='100%'
        w='100%'
        display='flex'
        alignItems='center'
        justifyContent='center'
        flexDirection='column'
        gap={2}
      >
        <Text color='whiteAlpha.400' fontSize='13px'>
          No log entries to display
        </Text>
        <Text color='whiteAlpha.300' fontSize='11px'>
          Logs will appear here as they are generated
        </Text>
      </Box>
    )
  }

  return (
    <Box
      ref={containerRef}
      position='absolute'
      top={0}
      left={0}
      right={0}
      bottom={0}
    >
      {dimensions.height > 0 && (
        <List<RowProps>
          listRef={setListRef}
          rowCount={entries.length}
          rowHeight={dynamicRowHeight}
          overscanCount={10}
          style={{ overflowX: 'hidden', height: dimensions.height, width: dimensions.width || '100%' }}
          rowComponent={Row}
          rowProps={{
            entries,
            expandedIds,
            onToggleExpand,
            searchText,
          }}
        />
      )}
    </Box>
  )
}

export const LogViewerList = memo(LogViewerListComponent)
