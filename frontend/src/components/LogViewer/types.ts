export type LogLevel = 'ERROR' | 'WARN' | 'INFO' | 'DEBUG' | 'TRACE'

export interface LogEntry {
  id: number
  raw: string
  timestamp: string | null
  date: string | null
  time: string | null
  level: LogLevel | null
  module: string | null
  message: string
  is_parsed: boolean
}

export interface LogInfo {
  log_path: string
  log_size: number
  exists: boolean
}

export interface LogFileInfo {
  filename: string
  path: string
  size: number
  created_at: string
  age_days: number
  is_current: boolean
}

export interface LogSettings {
  retention_count: number
  retention_days: number
}

export interface LogFilter {
  levels: LogLevel[]
  modules: string[]
  searchText: string
}

export interface LogRowProps {
  entry: LogEntry
  isExpanded: boolean
  onToggle: () => void
  onHeightChange?: (id: number, height: number) => void
  style: React.CSSProperties
  searchText?: string
}

export interface LogViewerListProps {
  entries: LogEntry[]
  expandedIds: Set<number>
  onToggleExpand: (id: number) => void
  searchText?: string
  autoFollow?: boolean
}

export interface LevelFilterDropdownProps {
  selectedLevels: LogLevel[]
  onLevelChange: (levels: LogLevel[]) => void
}

export interface ModuleFilterDropdownProps {
  availableModules: string[]
  selectedModules: string[]
  onModuleChange: (modules: string[]) => void
}

export interface FilterChipsProps {
  selectedLevels: LogLevel[]
  selectedModules: string[]
  searchText: string
  onRemoveLevel: (level: LogLevel) => void
  onRemoveModule: (module: string) => void
  onClearSearch: () => void
  onClearAll: () => void
}

export interface LogViewerToolbarProps {
  filter: LogFilter
  availableModules: string[]
  autoRefresh: boolean
  isFollowDisabled?: boolean
  onFilterChange: (filter: LogFilter) => void
  onAutoRefreshChange: (enabled: boolean) => void
  onClear: () => void
  onExport: () => void
  onCopy: () => void
  onOpenFolder: () => void
  isExporting: boolean
}
