import { queryOptions } from '@tanstack/react-query'
import { invoke } from '@tauri-apps/api/core'

import type { LogFileInfo, LogSettings } from '@/components/LogViewer'

import type { AppMode, McpStatus, SslSettingsData } from './types'

export const settingsQuery = queryOptions({
  queryKey: ['settings'],
  queryFn: () => invoke<Record<string, string>>('get_settings'),
})

export const sslSettingsQuery = queryOptions({
  queryKey: ['ssl-settings'],
  queryFn: () => invoke<SslSettingsData>('get_ssl_settings'),
})

export const logSettingsQuery = queryOptions({
  queryKey: ['log-settings'],
  queryFn: () => invoke<LogSettings>('get_log_settings'),
})

export const appModeQuery = queryOptions({
  queryKey: ['app-mode'],
  queryFn: () => invoke<AppMode>('get_app_mode_cmd'),
})

export const mcpStatusQuery = queryOptions({
  queryKey: ['mcp-status'],
  queryFn: () => invoke<McpStatus>('get_mcp_server_status'),
})

export const logFilesQuery = queryOptions({
  queryKey: ['log-files'],
  queryFn: () => invoke<LogFileInfo[]>('list_log_files'),
})
