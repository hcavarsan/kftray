import type { LogSettings } from '@/components/LogViewer'

export type AppMode = 'tray' | 'window'

export interface McpStatus {
  enabled: string
  port: string
  running: string
}

export interface SslSettingsData {
  ssl_enabled: boolean
  ssl_cert_validity_days: number
}

export interface SettingsDraft {
  disconnectTimeout: string
  networkMonitor: boolean
  autoUpdateEnabled: boolean
  sslEnabled: boolean
  sslCertValidityDays: string
  logRetentionCount: string
  logRetentionDays: string
  appMode: AppMode
  mcpEnabled: boolean
  mcpPort: string
}

export function buildSettingsDraft(data: {
  settings: Record<string, string>
  ssl?: SslSettingsData
  log?: LogSettings
  appMode: AppMode
  mcp: McpStatus
}): SettingsDraft {
  return {
    disconnectTimeout: data.settings.disconnect_timeout_minutes || '0',
    networkMonitor: data.settings.network_monitor === 'true',
    autoUpdateEnabled: data.settings.auto_update_enabled === 'true',
    sslEnabled: data.ssl?.ssl_enabled ?? false,
    sslCertValidityDays: String(data.ssl?.ssl_cert_validity_days ?? 365),
    logRetentionCount: String(data.log?.retention_count ?? 10),
    logRetentionDays: String(data.log?.retention_days ?? 7),
    appMode: data.appMode,
    mcpEnabled: data.mcp.enabled === 'true',
    mcpPort: data.mcp.port || '3000',
  }
}
