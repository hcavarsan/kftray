import { queryOptions } from '@tanstack/react-query'

import type { LogSettings } from '@/components/LogViewer'
import { invoke } from '@/lib/tauri'

import type { AppMode, McpStatus, SslSettingsData } from './types'

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

const sslSettingsQuery = queryOptions({
  queryKey: ['ssl-settings'],
  queryFn: () => invoke<SslSettingsData>('get_ssl_settings'),
  gcTime: 0,
  retry: false,
  meta: { errorToast: generalLoadToast },
})

const logSettingsQuery = queryOptions({
  queryKey: ['log-settings'],
  queryFn: () => invoke<LogSettings>('get_log_settings'),
  gcTime: 0,
  retry: false,
  meta: { errorToast: generalLoadToast },
})

const appModeQuery = queryOptions({
  queryKey: ['app-mode'],
  queryFn: () => invoke<AppMode>('get_app_mode_cmd'),
  gcTime: 0,
  meta: { errorToast: windowLoadToast },
})

const mcpStatusQuery = queryOptions({
  queryKey: ['mcp-status'],
  queryFn: () => invoke<McpStatus>('get_mcp_server_status'),
  gcTime: 0,
  meta: { errorToast: windowLoadToast },
})

export const SETTINGS_QUERIES = [
  settingsQuery,
  sslSettingsQuery,
  logSettingsQuery,
  appModeQuery,
  mcpStatusQuery,
] as const
