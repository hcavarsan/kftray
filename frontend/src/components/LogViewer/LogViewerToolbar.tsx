import { memo, useCallback } from 'react'
import {
  Copy,
  Download,
  FolderOpen,
  Pause,
  Play,
  Search,
  Trash2,
} from 'lucide-react'

import { Box, Flex, IconButton, Input, Spinner } from '@chakra-ui/react'

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
    [filter, onFilterChange],
  )

  const handleLevelChange = useCallback(
    (levels: LogLevel[]) => {
      onFilterChange({ ...filter, levels })
    },
    [filter, onFilterChange],
  )

  const handleModuleChange = useCallback(
    (modules: string[]) => {
      onFilterChange({ ...filter, modules })
    },
    [filter, onFilterChange],
  )

  const handleRemoveLevel = useCallback(
    (level: LogLevel) => {
      onFilterChange({
        ...filter,
        levels: filter.levels.filter(l => l !== level),
      })
    },
    [filter, onFilterChange],
  )

  const handleRemoveModule = useCallback(
    (module: string) => {
      onFilterChange({
        ...filter,
        modules: filter.modules.filter(m => m !== module),
      })
    },
    [filter, onFilterChange],
  )

  const handleClearSearch = useCallback(() => {
    onFilterChange({ ...filter, searchText: '' })
  }, [filter, onFilterChange])

  const handleClearAll = useCallback(() => {
    onFilterChange({ levels: [], modules: [], searchText: '' })
  }, [onFilterChange])

  return (
    <Box borderBottom='1px solid' borderBottomColor='border.subtle' pb={2}>
      <Flex align='center' gap={2} flexWrap='wrap'>
        <IconButton
          aria-label={autoRefresh ? 'Stop following' : 'Follow logs'}
          size='sm'
          variant='ghost'
          h='28px'
          w='28px'
          minW='28px'
          borderRadius='4px'
          border='1px solid'
          borderColor={autoRefresh ? 'accent.emphasized' : 'border'}
          bg={autoRefresh ? 'accent.subtle' : 'transparent'}
          color={autoRefresh ? 'accent.fg' : 'fg.subtle'}
          _hover={{
            bg: autoRefresh ? 'accent.muted' : 'bg.faint',
            borderColor: autoRefresh
              ? 'accent.emphasized'
              : 'border.emphasized',
          }}
          onClick={() => onAutoRefreshChange(!autoRefresh)}
          title={autoRefresh ? 'Stop following' : 'Follow logs'}
          disabled={isFollowDisabled}
          opacity={isFollowDisabled ? 0.4 : 1}
        >
          {autoRefresh ? <Pause size={14} /> : <Play size={14} />}
        </IconButton>

        <Flex
          align='center'
          flex={1}
          minW='180px'
          maxW='300px'
          position='relative'
        >
          <Box
            position='absolute'
            left={2}
            color='fg.faint'
            pointerEvents='none'
            zIndex={1}
          >
            <Search size={11} />
          </Box>
          <Input
            placeholder='Search logs...'
            size='sm'
            value={filter.searchText}
            onChange={handleSearchChange}
            pl={6}
            height='24px'
            fontSize='11px'
            bg='bg.raised'
            border='1px solid'
            borderColor='border'
            color='fg'
            _placeholder={{ color: 'fg.faint' }}
            _hover={{ borderColor: 'border.emphasized' }}
            _focus={{ borderColor: 'accent.focusRing', boxShadow: 'none' }}
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
            aria-label='Copy logs'
            size='xs'
            variant='ghost'
            onClick={onCopy}
            title='Copy all logs'
            h='24px'
            w='24px'
            minW='24px'
            color='fg.subtle'
            _hover={{ bg: 'bg.hover', color: 'fg' }}
          >
            <Copy size={12} />
          </IconButton>
          <IconButton
            aria-label='Export diagnostic report'
            size='xs'
            variant='ghost'
            onClick={onExport}
            disabled={isExporting}
            title='Export report'
            h='24px'
            w='24px'
            minW='24px'
            color='fg.subtle'
            _hover={{ bg: 'bg.hover', color: 'fg' }}
          >
            {isExporting ? <Spinner size='xs' /> : <Download size={12} />}
          </IconButton>
          <IconButton
            aria-label='Open log folder'
            size='xs'
            variant='ghost'
            onClick={onOpenFolder}
            title='Open folder'
            h='24px'
            w='24px'
            minW='24px'
            color='fg.subtle'
            _hover={{ bg: 'bg.hover', color: 'fg' }}
          >
            <FolderOpen size={12} />
          </IconButton>
          <IconButton
            aria-label='Clear logs'
            size='xs'
            variant='ghost'
            onClick={onClear}
            title='Clear logs'
            h='24px'
            w='24px'
            minW='24px'
            color='fg.subtle'
            _hover={{ bg: 'danger.subtle', color: 'danger.fg' }}
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
