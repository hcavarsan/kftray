import { memo, type ReactNode, useEffect, useRef, useState } from 'react'
import { Check, ChevronRight, Copy } from 'lucide-react'

import { Box, Flex, IconButton, Text } from '@chakra-ui/react'

import { useCopyToClipboard } from '@/hooks/useCopyToClipboard'

import { LEVEL_COLORS } from './constants'
import type { LogEntry, LogLevel, LogRowProps } from './types'
import { highlightText } from './utils/filterLogs'

const EXPAND_TRANSITION = '180ms cubic-bezier(0.2, 0, 0, 1)'

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
      flexShrink={0}
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
  searchText: string
}) {
  if (!searchText.trim()) {
    return text
  }

  return highlightText(text, searchText).map((segment, index) =>
    segment.isMatch ? (
      <Box
        as='mark'
        // biome-ignore lint/suspicious/noArrayIndexKey: segments are positional and never reorder
        key={index}
        bg='search.match'
        color='fg'
        px={0.5}
        borderRadius='2px'
      >
        {segment.text}
      </Box>
    ) : (
      // biome-ignore lint/suspicious/noArrayIndexKey: segments are positional and never reorder
      <span key={index}>{segment.text}</span>
    ),
  )
}

function DetailField({
  label,
  children,
}: {
  label: string
  children: ReactNode
}) {
  return (
    <>
      <Text color='fg.subtle' fontSize='10px' lineHeight='18px'>
        {label}
      </Text>
      <Box color='fg' minW={0} lineHeight='18px'>
        {children}
      </Box>
    </>
  )
}

function ExpandedDetails({ entry }: { entry: LogEntry }) {
  const levelColors = entry.level ? LEVEL_COLORS[entry.level] : null
  const [copied, setCopied] = useState(false)
  const copiedTimeout = useRef<number>(undefined)
  const { mutate: copyText } = useCopyToClipboard()

  useEffect(() => () => clearTimeout(copiedTimeout.current), [])

  const copyEntry = () => {
    const text = entry.is_parsed
      ? [
          `Timestamp: ${entry.timestamp}`,
          `Level: ${entry.level}`,
          `Module: ${entry.module}`,
          `Message: ${entry.message}`,
        ].join('\n')
      : entry.raw

    copyText(text, {
      onSuccess: () => {
        setCopied(true)
        clearTimeout(copiedTimeout.current)
        copiedTimeout.current = window.setTimeout(() => setCopied(false), 1500)
      },
    })
  }

  return (
    <Box
      position='relative'
      ml='28px'
      mr={3}
      mt={0.5}
      mb={3}
      px={3.5}
      py={3}
      pr={10}
      bg='bg.surface'
      borderRadius='6px'
      border='1px solid'
      borderColor='border.subtle'
      borderLeft='2px solid'
      borderLeftColor={levelColors?.border ?? 'border'}
      fontSize='11px'
      fontFamily='mono'
    >
      <Box
        display='grid'
        gridTemplateColumns='max-content minmax(0, 1fr)'
        columnGap={4}
        rowGap={1.5}
      >
        {entry.timestamp && (
          <DetailField label='Timestamp'>{entry.timestamp}</DetailField>
        )}
        {entry.level && (
          <DetailField label='Level'>
            <Text as='span' color={levelColors?.text} fontWeight='medium'>
              {entry.level}
            </Text>
          </DetailField>
        )}
        {entry.module && (
          <DetailField label='Module'>
            <Text as='span' color='log.module.fg' wordBreak='break-all'>
              {entry.module}
            </Text>
          </DetailField>
        )}
        <DetailField label='Message'>
          <Text as='span' whiteSpace='pre-wrap' overflowWrap='anywhere'>
            {entry.message}
          </Text>
        </DetailField>
      </Box>

      <IconButton
        aria-label={copied ? 'Copied' : 'Copy entry'}
        title={copied ? 'Copied' : 'Copy entry'}
        position='absolute'
        top={2}
        right={2}
        size='2xs'
        variant='ghost'
        minW='22px'
        h='22px'
        color={copied ? 'success.fg' : 'fg.subtle'}
        _hover={{ bg: 'bg.hover', color: copied ? 'success.fg' : 'fg' }}
        onClick={copyEntry}
      >
        {copied ? <Check size={12} /> : <Copy size={12} />}
      </IconButton>
    </Box>
  )
}

function LogRowComponent({
  entry,
  isExpanded,
  onToggle,
  searchText,
}: LogRowProps) {
  const levelColors = entry.level ? LEVEL_COLORS[entry.level] : null

  // Details mount on first expand and stay mounted so collapsing can animate.
  const [hasDetails, setHasDetails] = useState(isExpanded)

  if (isExpanded && !hasDetails) {
    setHasDetails(true)
  }

  return (
    <Box
      borderBottom='1px solid'
      borderBottomColor='border.subtle'
      bg={isExpanded ? 'bg.faint' : 'transparent'}
      transition='background-color 120ms'
    >
      <Flex
        role='button'
        tabIndex={0}
        aria-expanded={isExpanded}
        align='center'
        gap={2}
        h='32px'
        px={2}
        cursor='pointer'
        userSelect='none'
        _hover={{ bg: 'bg.faint' }}
        _focusVisible={{
          outline: '1px solid',
          outlineColor: 'accent.focusRing',
          outlineOffset: '-1px',
        }}
        onClick={() => onToggle(entry.id)}
        onKeyDown={event => {
          if (event.key === 'Enter' || event.key === ' ') {
            event.preventDefault()
            onToggle(entry.id)
          }
        }}
      >
        <Box
          as={ChevronRight}
          width='12px'
          height='12px'
          color='fg.faint'
          flexShrink={0}
          transform={isExpanded ? 'rotate(90deg)' : undefined}
          transition={`transform ${EXPAND_TRANSITION}`}
          _motionReduce={{ transition: 'none' }}
        />

        {entry.time && (
          <Text
            fontSize='11px'
            fontFamily='mono'
            color='fg.subtle'
            flexShrink={0}
            minW='60px'
          >
            {entry.time}
          </Text>
        )}

        {entry.level && <LevelBadge level={entry.level} />}

        {entry.module && (
          <Text
            fontSize='11px'
            fontFamily='mono'
            color='log.module.fg'
            flexShrink={0}
            maxW='180px'
            truncate
          >
            <HighlightedText text={entry.module} searchText={searchText} />
          </Text>
        )}

        <Text
          fontSize='11px'
          fontFamily='mono'
          color={entry.is_parsed ? 'fg.secondary' : 'fg.subtle'}
          flex={1}
          truncate
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

      <Box
        display='grid'
        gridTemplateRows={isExpanded ? '1fr' : '0fr'}
        transition={`grid-template-rows ${EXPAND_TRANSITION}`}
        _motionReduce={{ transition: 'none' }}
        inert={!isExpanded}
      >
        <Box overflow='hidden' minH={0}>
          {hasDetails && <ExpandedDetails entry={entry} />}
        </Box>
      </Box>
    </Box>
  )
}

export const LogRow = memo(LogRowComponent)
