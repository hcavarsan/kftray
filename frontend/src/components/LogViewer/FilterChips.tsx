import { memo } from 'react'
import { X } from 'lucide-react'

import { Box, Flex, Text } from '@chakra-ui/react'

import { COLORS, LEVEL_COLORS } from './constants'
import type { FilterChipsProps, LogLevel } from './types'

function Chip({
  label,
  bg,
  color,
  borderColor,
  onRemove,
}: {
  label: string
  bg: string
  color: string
  borderColor: string
  onRemove: () => void
}) {
  return (
    <Flex
      align='center'
      gap={1}
      px={1.5}
      py={0.5}
      bg={bg}
      color={color}
      border='1px solid'
      borderColor={borderColor}
      borderRadius='4px'
      fontSize='10px'
      fontFamily='mono'
    >
      <Text>{label}</Text>
      <Box
        as='button'
        display='flex'
        alignItems='center'
        justifyContent='center'
        w='12px'
        h='12px'
        borderRadius='2px'
        cursor='pointer'
        opacity={0.7}
        _hover={{ opacity: 1, bg: 'rgba(0, 0, 0, 0.2)' }}
        onClick={e => {
          e.stopPropagation()
          onRemove()
        }}
      >
        <X size={8} />
      </Box>
    </Flex>
  )
}

function FilterChipsComponent({
  selectedLevels,
  selectedModules,
  searchText,
  onRemoveLevel,
  onRemoveModule,
  onClearSearch,
  onClearAll,
}: FilterChipsProps) {
  const hasFilters =
    selectedLevels.length > 0 || selectedModules.length > 0 || searchText.trim()

  if (!hasFilters) {
    return null
  }

  return (
    <Flex align='center' gap={1.5} flexWrap='wrap' pt={2}>
      {/* Level chips */}
      {selectedLevels.map(level => {
        const colors = LEVEL_COLORS[level as LogLevel]

        return (
          <Chip
            key={`level-${level}`}
            label={level}
            bg={colors.bg}
            color={colors.text}
            borderColor={colors.border}
            onRemove={() => onRemoveLevel(level)}
          />
        )
      })}

      {/* Module chips */}
      {selectedModules.map(module => {
        const formatModuleName = (mod: string): string => {
          const formatted = mod
            .replace(/^kftray_portforward::/, '')
            .replace(/^kftray_tauri::/, '')
            .replace(/^kftray_/, '')
          const segments = formatted.split('::')

          if (segments.length > 2) {
            return '… ' + segments.slice(-2).join(' › ')
          }

          return segments.join(' › ')
        }
        const displayName = formatModuleName(module)

        return (
          <Chip
            key={`module-${module}`}
            label={displayName}
            bg='rgba(34, 211, 238, 0.1)'
            color={COLORS.accentCyan}
            borderColor='rgba(34, 211, 238, 0.2)'
            onRemove={() => onRemoveModule(module)}
          />
        )
      })}

      {/* Search chip */}
      {searchText.trim() && (
        <Chip
          label={`"${searchText.length > 15 ? searchText.slice(0, 12) + '...' : searchText}"`}
          bg='rgba(251, 191, 36, 0.1)'
          color='rgba(251, 191, 36, 1)'
          borderColor='rgba(251, 191, 36, 0.2)'
          onRemove={onClearSearch}
        />
      )}

      {/* Clear all button */}
      {(selectedLevels.length + selectedModules.length > 1 ||
        (selectedLevels.length + selectedModules.length >= 1 &&
          searchText.trim())) && (
        <Text
          as='button'
          fontSize='10px'
          color='whiteAlpha.400'
          cursor='pointer'
          ml={1}
          _hover={{ color: 'whiteAlpha.700', textDecoration: 'underline' }}
          onClick={onClearAll}
        >
          Clear all
        </Text>
      )}
    </Flex>
  )
}

export const FilterChips = memo(FilterChipsComponent)
