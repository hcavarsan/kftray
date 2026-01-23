import { memo, useCallback } from 'react'
import { Copy, Download, FolderOpen, Pause, Play, Search, Trash2 } from 'lucide-react'

import { Box, Flex, IconButton, Input, Spinner } from '@chakra-ui/react'

import { COLORS } from './constants'
import { FilterChips } from './FilterChips'
import { LevelFilterDropdown } from './LevelFilterDropdown'
import { ModuleFilterDropdown } from './ModuleFilterDropdown'
import type { LogLevel, LogViewerToolbarProps } from './types'

function LogViewerToolbarComponent({
  filter,
  availableModules,
  autoRefresh,
  isFollowDisabled = false,
  onFilterChange,
  onAutoRefreshChange,
  onClear,
  onExport,
  onCopy,
  onOpenFolder,
  isExporting,
}: LogViewerToolbarProps) {
  const handleSearchChange = useCallback(
    (e: React.ChangeEvent<HTMLInputElement>) => {
      onFilterChange({ ...filter, searchText: e.target.value })
    },
    [filter, onFilterChange]
  )

  const handleLevelChange = useCallback(
    (levels: LogLevel[]) => {
      onFilterChange({ ...filter, levels })
    },
    [filter, onFilterChange]
  )

  const handleModuleChange = useCallback(
    (modules: string[]) => {
      onFilterChange({ ...filter, modules })
    },
    [filter, onFilterChange]
  )

  const handleRemoveLevel = useCallback(
    (level: LogLevel) => {
      onFilterChange({ ...filter, levels: filter.levels.filter(l => l !== level) })
    },
    [filter, onFilterChange]
  )

  const handleRemoveModule = useCallback(
    (module: string) => {
      onFilterChange({ ...filter, modules: filter.modules.filter(m => m !== module) })
    },
    [filter, onFilterChange]
  )

  const handleClearSearch = useCallback(() => {
    onFilterChange({ ...filter, searchText: '' })
  }, [filter, onFilterChange])

  const handleClearAll = useCallback(() => {
    onFilterChange({ levels: [], modules: [], searchText: '' })
  }, [onFilterChange])

  return (
    <Box borderBottom="1px solid" borderBottomColor={COLORS.borderSubtle} pb={2}>
      <Flex align="center" gap={2} flexWrap="wrap">
        <IconButton
          aria-label={autoRefresh ? 'Stop following' : 'Follow logs'}
          size="sm"
          variant="ghost"
          h="28px"
          w="28px"
          minW="28px"
          borderRadius="4px"
          border="1px solid"
          borderColor={autoRefresh ? 'rgba(59, 130, 246, 0.4)' : COLORS.borderDefault}
          bg={autoRefresh ? 'rgba(59, 130, 246, 0.15)' : 'transparent'}
          color={autoRefresh ? COLORS.accentBlue : 'whiteAlpha.600'}
          _hover={{
            bg: autoRefresh ? 'rgba(59, 130, 246, 0.2)' : 'whiteAlpha.50',
            borderColor: autoRefresh ? 'rgba(59, 130, 246, 0.5)' : COLORS.borderHover,
          }}
          onClick={() => onAutoRefreshChange(!autoRefresh)}
          title={autoRefresh ? 'Stop following' : 'Follow logs'}
          disabled={isFollowDisabled}
          opacity={isFollowDisabled ? 0.4 : 1}
        >
          {autoRefresh ? <Pause size={14} /> : <Play size={14} />}
        </IconButton>

        <Flex align="center" flex={1} minW="180px" maxW="300px" position="relative">
          <Box position="absolute" left={2} color="whiteAlpha.400" pointerEvents="none" zIndex={1}>
            <Search size={11} />
          </Box>
          <Input
            placeholder="Search logs..."
            size="sm"
            value={filter.searchText}
            onChange={handleSearchChange}
            pl={6}
            height="24px"
            fontSize="11px"
            bg={COLORS.bgInput}
            border="1px solid"
            borderColor={COLORS.borderDefault}
            color="whiteAlpha.900"
            _placeholder={{ color: 'whiteAlpha.400' }}
            _hover={{ borderColor: COLORS.borderHover }}
            _focus={{ borderColor: COLORS.accentBlue, boxShadow: 'none' }}
          />
        </Flex>

        <LevelFilterDropdown
          selectedLevels={filter.levels}
          onLevelChange={handleLevelChange}
        />
        <ModuleFilterDropdown
          availableModules={availableModules}
          selectedModules={filter.modules}
          onModuleChange={handleModuleChange}
        />

        <Box flex={1} />

        <Flex gap={0.5}>
          <IconButton
            aria-label="Copy logs"
            size="xs"
            variant="ghost"
            onClick={onCopy}
            title="Copy all logs"
            h="24px"
            w="24px"
            minW="24px"
            color="whiteAlpha.600"
            _hover={{ bg: 'whiteAlpha.100', color: 'whiteAlpha.900' }}
          >
            <Copy size={12} />
          </IconButton>
          <IconButton
            aria-label="Export diagnostic report"
            size="xs"
            variant="ghost"
            onClick={onExport}
            disabled={isExporting}
            title="Export report"
            h="24px"
            w="24px"
            minW="24px"
            color="whiteAlpha.600"
            _hover={{ bg: 'whiteAlpha.100', color: 'whiteAlpha.900' }}
          >
            {isExporting ? <Spinner size="xs" /> : <Download size={12} />}
          </IconButton>
          <IconButton
            aria-label="Open log folder"
            size="xs"
            variant="ghost"
            onClick={onOpenFolder}
            title="Open folder"
            h="24px"
            w="24px"
            minW="24px"
            color="whiteAlpha.600"
            _hover={{ bg: 'whiteAlpha.100', color: 'whiteAlpha.900' }}
          >
            <FolderOpen size={12} />
          </IconButton>
          <IconButton
            aria-label="Clear logs"
            size="xs"
            variant="ghost"
            onClick={onClear}
            title="Clear logs"
            h="24px"
            w="24px"
            minW="24px"
            color="whiteAlpha.600"
            _hover={{ bg: 'rgba(229, 62, 62, 0.1)', color: 'red.300' }}
          >
            <Trash2 size={12} />
          </IconButton>
        </Flex>
      </Flex>

      <FilterChips
        selectedLevels={filter.levels}
        selectedModules={filter.modules}
        searchText={filter.searchText}
        onRemoveLevel={handleRemoveLevel}
        onRemoveModule={handleRemoveModule}
        onClearSearch={handleClearSearch}
        onClearAll={handleClearAll}
      />
    </Box>
  )
}

export const LogViewerToolbar = memo(LogViewerToolbarComponent)
