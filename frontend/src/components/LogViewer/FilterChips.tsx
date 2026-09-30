import { memo } from 'react'
import { X } from 'lucide-react'

import { Box, Flex, Text } from '@chakra-ui/react'

import { LEVEL_COLORS } from './constants'
import type { FilterChipsProps } from './types'

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
        _hover={{ opacity: 1, bg: 'bg.shade' }}
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
      {selectedLevels.map(level => {
        const colors = LEVEL_COLORS[level]

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

      {selectedModules.map(module => {
        const formatModuleName = (mod: string): string => {
          const formatted = mod
            .replace(/^kftray_portforward::/, '')
            .replace(/^kftray_tauri::/, '')
            .replace(/^kftray_/, '')
          const segments = formatted.split('::')

          if (segments.length > 2) {
            return `… ${segments.slice(-2).join(' › ')}`
          }

          return segments.join(' › ')
        }
        const displayName = formatModuleName(module)

        return (
          <Chip
            key={`module-${module}`}
            label={displayName}
            bg='log.module.subtle'
            color='log.module.fg'
            borderColor='log.module.muted'
            onRemove={() => onRemoveModule(module)}
          />
        )
      })}

      {searchText.trim() && (
        <Chip
          label={`"${searchText.length > 15 ? `${searchText.slice(0, 12)}...` : searchText}"`}
          bg='search.bg'
          color='log.warn.text'
          borderColor='search.border'
          onRemove={onClearSearch}
        />
      )}

      {(selectedLevels.length + selectedModules.length > 1 ||
        (selectedLevels.length + selectedModules.length >= 1 &&
          searchText.trim())) && (
        <Text
          as='button'
          fontSize='10px'
          color='fg.faint'
          cursor='pointer'
          ml={1}
          _hover={{ color: 'fg.muted', textDecoration: 'underline' }}
          onClick={onClearAll}
        >
          Clear all
        </Text>
      )}
    </Flex>
  )
}

export const FilterChips = memo(FilterChipsComponent)
