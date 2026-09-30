import { queryOptions } from '@tanstack/react-query'
import { invoke } from '@tauri-apps/api/core'

import type { LogFileInfo, LogSettings } from '@/components/LogViewer'

import type { AppMode, McpStatus, SslSettingsData } from './types'

const withDefault = async <T>(load: Promise<T>, fallback: T) => {
  try {
    return await load
  } catch {
    return fallback
  }
}

const generalLoadToast = {
  id: 'settings-load-error',
  title: 'Error',
  description: 'Failed to load settings',
  duration: 3000,
}

const windowLoadToast = {
  id: 'window-settings-load-error',
  title: 'Error',
  description: 'Failed to load window and MCP settings',
  duration: 3000,
}

export const settingsQuery = queryOptions({
  queryKey: ['settings'],
  queryFn: () => invoke<Record<string, string>>('get_settings'),
  gcTime: 0,
  meta: { errorToast: generalLoadToast },
})

export const sslSettingsQuery = queryOptions({
  queryKey: ['ssl-settings'],
  queryFn: () =>
    withDefault(invoke<SslSettingsData>('get_ssl_settings'), {
      ssl_enabled: false,
      ssl_cert_validity_days: 365,
    }),
  gcTime: 0,
  meta: { errorToast: generalLoadToast },
})

export const logSettingsQuery = queryOptions({
  queryKey: ['log-settings'],
  queryFn: () =>
    withDefault(invoke<LogSettings>('get_log_settings'), {
      retention_count: 10,
      retention_days: 7,
    }),
  gcTime: 0,
  meta: { errorToast: generalLoadToast },
})

export const appModeQuery = queryOptions({
  queryKey: ['app-mode'],
  queryFn: () => invoke<AppMode>('get_app_mode_cmd'),
  gcTime: 0,
  meta: { errorToast: windowLoadToast },
})

export const mcpStatusQuery = queryOptions({
  queryKey: ['mcp-status'],
  queryFn: () => invoke<McpStatus>('get_mcp_server_status'),
  gcTime: 0,
  meta: { errorToast: windowLoadToast },
})

export const logFilesQuery = queryOptions({
  queryKey: ['log-files'],
  queryFn: () => invoke<LogFileInfo[]>('list_log_files'),
})
