import { memo, useCallback, useEffect, useRef, useState } from 'react'
import { ChevronDown, ChevronRight, Copy } from 'lucide-react'

import { Box, Flex, IconButton, Text } from '@chakra-ui/react'

import { Tooltip } from '@/components/ui/tooltip'

import { highlightText } from './utils/filterLogs'
import { COLORS, LEVEL_COLORS } from './constants'
import type { LogLevel, LogRowProps } from './types'

function LevelBadge({ level }: { level: LogLevel }) {
  const colors = LEVEL_COLORS[level]

  return (
    <Box
      px={1.5}
      py={0.5}
      borderRadius='4px'
      fontSize='10px'
      fontWeight='medium'
      fontFamily='mono'
      bg={colors.bg}
      color={colors.text}
      border='1px solid'
      borderColor={colors.border}
      minW='44px'
      textAlign='center'
      letterSpacing='0.02em'
    >
      {level}
    </Box>
  )
}

function HighlightedText({
  text,
  searchText,
}: {
  text: string
  searchText?: string
}) {
  if (!searchText?.trim()) {
    return <>{text}</>
  }

  const segments = highlightText(text, searchText)

  return (
    <>
      {segments.map((segment, index) =>
        segment.isMatch ? (
          <Box
            as='mark'
            key={index}
            bg='rgba(251, 191, 36, 0.3)'
            color='white'
            px={0.5}
            borderRadius='2px'
          >
            {segment.text}
          </Box>
        ) : (
          <span key={index}>{segment.text}</span>
        ),
      )}
    </>
  )
}

function ExpandedDetails({ entry }: { entry: LogRowProps['entry'] }) {
  const levelColors = entry.level ? LEVEL_COLORS[entry.level as LogLevel] : null
  const [showCopied, setShowCopied] = useState(false)

  const handleCopyAll = useCallback(async () => {
    const parts = []

    if (entry.timestamp) {
      parts.push(`Timestamp: ${entry.timestamp}`)
    }
    if (entry.level) {
      parts.push(`Level: ${entry.level}`)
    }
    if (entry.module) {
      parts.push(`Module: ${entry.module}`)
    }
    parts.push(`Message: ${entry.message}`)
    try {
      await navigator.clipboard.writeText(parts.join('\n'))
      setShowCopied(true)
      setTimeout(() => setShowCopied(false), 1500)
    } catch (err) {
      console.error('Failed to copy:', err)
    }
  }, [entry])

  return (
    <Box
      mt={2}
      p={3}
      bg={COLORS.bgSecondary}
      borderRadius='4px'
      border='1px solid'
      borderColor={COLORS.borderDefault}
      borderLeft='2px solid'
      borderLeftColor={levelColors?.border ?? COLORS.borderDefault}
      fontSize='11px'
      fontFamily='mono'
    >
      <Flex direction='column' gap={2}>
        {entry.timestamp && (
          <Box>
            <Text color='whiteAlpha.500' display='inline' fontSize='10px'>
              Timestamp:{' '}
            </Text>
            <Text color='whiteAlpha.900' display='inline'>
              {entry.timestamp}
            </Text>
          </Box>
        )}

        {entry.level && (
          <Box>
            <Text color='whiteAlpha.500' display='inline' fontSize='10px'>
              Level:{' '}
            </Text>
            <Text
              color={levelColors?.text}
              display='inline'
              fontWeight='medium'
            >
              {entry.level}
            </Text>
          </Box>
        )}

        {entry.module && (
          <Box>
            <Text color='whiteAlpha.500' display='inline' fontSize='10px'>
              Module:{' '}
            </Text>
            <Text
              color={COLORS.accentCyan}
              display='inline'
              wordBreak='break-all'
            >
              {entry.module}
            </Text>
          </Box>
        )}

        <Flex align='flex-start' justify='space-between'>
          <Box flex={1} pr={2}>
            <Text color='whiteAlpha.500' display='inline' fontSize='10px'>
              Message:{' '}
            </Text>
            <Text color='whiteAlpha.900' display='inline' wordBreak='break-all'>
              {entry.message}
            </Text>
          </Box>
          <Tooltip content='Copied!' open={showCopied} portalled>
            <IconButton
              aria-label='Copy all'
              size='2xs'
              variant='ghost'
              onClick={handleCopyAll}
              minW='20px'
              h='20px'
              color='whiteAlpha.500'
              _hover={{ bg: 'whiteAlpha.100', color: 'whiteAlpha.800' }}
            >
              <Copy size={11} />
            </IconButton>
          </Tooltip>
        </Flex>
      </Flex>
    </Box>
  )
}

function LogRowComponent({
  entry,
  isExpanded,
  onToggle,
  onHeightChange,
  style,
  searchText,
}: LogRowProps) {
  const levelColors = entry.level ? LEVEL_COLORS[entry.level as LogLevel] : null
  const contentRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!isExpanded || !onHeightChange || !contentRef.current) {
      return
    }

    const measureHeight = () => {
      if (contentRef.current) {
        const height = contentRef.current.scrollHeight

        onHeightChange(entry.id, height)
      }
    }

    measureHeight()

    const observer = new ResizeObserver(measureHeight)

    observer.observe(contentRef.current)

    return () => observer.disconnect()
  }, [isExpanded, entry.id, entry.message, onHeightChange])

  return (
    <Box
      ref={contentRef}
      style={style}
      px={2}
      py={1}
      borderBottom='1px solid'
      borderBottomColor={COLORS.borderSubtle}
      bg={isExpanded ? 'rgba(255, 255, 255, 0.02)' : 'transparent'}
      _hover={{ bg: 'rgba(255, 255, 255, 0.03)' }}
      transition='background 0.1s'
      overflow='hidden'
    >
      <Flex align='center' gap={2} cursor='pointer' onClick={onToggle} h='28px'>
        <Box color='whiteAlpha.400' flexShrink={0}>
          {isExpanded ? <ChevronDown size={12} /> : <ChevronRight size={12} />}
        </Box>

        {entry.time && (
          <Text
            fontSize='11px'
            fontFamily='mono'
            color='whiteAlpha.500'
            flexShrink={0}
            minW='60px'
          >
            {entry.time}
          </Text>
        )}

        {entry.level && (
          <Box flexShrink={0}>
            <LevelBadge level={entry.level as LogLevel} />
          </Box>
        )}

        {entry.module && (
          <Text
            fontSize='11px'
            fontFamily='mono'
            color={COLORS.accentCyan}
            flexShrink={0}
            maxW='180px'
            overflow='hidden'
            textOverflow='ellipsis'
            whiteSpace='nowrap'
          >
            <HighlightedText text={entry.module} searchText={searchText} />
          </Text>
        )}

        <Text
          fontSize='11px'
          fontFamily='mono'
          color={entry.is_parsed ? 'whiteAlpha.800' : 'whiteAlpha.500'}
          flex={1}
          overflow='hidden'
          textOverflow='ellipsis'
          whiteSpace='nowrap'
        >
          <HighlightedText text={entry.message} searchText={searchText} />
        </Text>

        {levelColors && (
          <Box
            w='2px'
            h='14px'
            bg={levelColors.border}
            borderRadius='1px'
            flexShrink={0}
          />
        )}
      </Flex>

      {isExpanded && <ExpandedDetails entry={entry} />}
    </Box>
  )
}

export const LogRow = memo(LogRowComponent)
