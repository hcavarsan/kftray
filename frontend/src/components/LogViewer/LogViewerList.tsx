import {
  memo,
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from 'react'
import { VariableSizeList as List } from 'react-window'

import { Box, Text } from '@chakra-ui/react'

import { ROW_HEIGHT_COLLAPSED, ROW_HEIGHT_EXPANDED_DEFAULT } from './constants'
import { LogRow } from './LogRow'
import type { LogViewerListProps } from './types'

function LogViewerListComponent({
  entries,
  expandedIds,
  onToggleExpand,
  searchText,
  autoFollow = false,
}: LogViewerListProps) {
  const listRef = useRef<List>(null)
  const containerRef = useRef<HTMLDivElement>(null)
  const [dimensions, setDimensions] = useState({ width: 0, height: 0 })
  const prevEntriesLengthRef = useRef(entries.length)

  const measuredHeights = useRef<Map<number, number>>(new Map())

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
      const currentHeight = measuredHeights.current.get(id)

      if (currentHeight !== height) {
        measuredHeights.current.set(id, height)
        const index = entries.findIndex(e => e.id === id)

        if (index !== -1 && listRef.current) {
          listRef.current.resetAfterIndex(index)
        }
      }
    },
    [entries],
  )

  const getItemSize = useCallback(
    (index: number) => {
      const entry = entries[index]

      if (!entry) {
        return ROW_HEIGHT_COLLAPSED
      }
      if (!expandedIds.has(entry.id)) {
        return ROW_HEIGHT_COLLAPSED
      }

      return (
        measuredHeights.current.get(entry.id) ?? ROW_HEIGHT_EXPANDED_DEFAULT
      )
    },
    [entries, expandedIds],
  )

  useEffect(() => {
    if (listRef.current) {
      listRef.current.resetAfterIndex(0)
    }
  }, [expandedIds])

  useEffect(() => {
    if (autoFollow && listRef.current && entries.length > 0) {
      requestAnimationFrame(() => {
        listRef.current?.scrollToItem(entries.length - 1, 'end')
      })
    }
    prevEntriesLengthRef.current = entries.length
  }, [entries.length, autoFollow])

  useEffect(() => {
    if (autoFollow && listRef.current && entries.length > 0) {
      listRef.current.scrollToItem(entries.length - 1, 'end')
    }
  }, [autoFollow, entries.length])

  const Row = useCallback(
    ({ index, style }: { index: number; style: React.CSSProperties }) => {
      const entry = entries[index]

      if (!entry) {
        return null
      }

      const isExpanded = expandedIds.has(entry.id)

      return (
        <LogRow
          entry={entry}
          isExpanded={isExpanded}
          onToggle={() => onToggleExpand(entry.id)}
          onHeightChange={handleHeightChange}
          style={style}
          searchText={searchText}
        />
      )
    },
    [entries, expandedIds, onToggleExpand, handleHeightChange, searchText],
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
        <List
          ref={listRef}
          height={dimensions.height}
          width={dimensions.width || '100%'}
          itemCount={entries.length}
          itemSize={getItemSize}
          overscanCount={10}
          style={{ overflowX: 'hidden' }}
        >
          {Row}
        </List>
      )}
    </Box>
  )
}

export const LogViewerList = memo(LogViewerListComponent)
