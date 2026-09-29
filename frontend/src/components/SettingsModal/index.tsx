import { useState } from 'react'

import { Box, Dialog, Flex, Spinner, Stack, Text } from '@chakra-ui/react'
import { useQueries, useQueryClient } from '@tanstack/react-query'
import { invoke } from '@tauri-apps/api/core'

import type { LogSettings as LogSettingsData } from '@/components/LogViewer'
import { Button } from '@/components/ui/button'
import { AppDialog } from '@/components/ui/dialog'
import { toaster } from '@/components/ui/toaster'
import { errorMessage } from '@/lib/errors'

import { McpServerSettings } from './McpServerSettings'
import {
  appModeQuery,
  logSettingsQuery,
  mcpStatusQuery,
  settingsQuery,
  sslSettingsQuery,
} from './queries'
import { LogSettings } from './sections/LogSettings'
import { NetworkSettings } from './sections/NetworkSettings'
import { SslSettings } from './sections/SslSettings'
import { UpdateSettings } from './sections/UpdateSettings'
import {
  type AppMode,
  buildSettingsDraft,
  type McpStatus,
  type SettingsDraft,
  type SslSettingsData,
} from './types'
import { WindowSettings } from './WindowSettings'

const SETTINGS_QUERIES = [
  settingsQuery,
  sslSettingsQuery,
  logSettingsQuery,
  appModeQuery,
  mcpStatusQuery,
] as const

const inRange = (value: string, min: number, max: number) => {
  const parsed = parseInt(value, 10)

  return !Number.isNaN(parsed) && parsed >= min && parsed <= max
}

function validationError(draft: SettingsDraft): string | null {
  if (!inRange(draft.disconnectTimeout, 0, Number.MAX_SAFE_INTEGER)) {
    return 'Please enter a valid number (0 or greater) for timeout'
  }
  if (!inRange(draft.sslCertValidityDays, 1, 3650)) {
    return 'Certificate validity must be between 1 and 3650 days'
  }
  if (!inRange(draft.logRetentionCount, 1, 100)) {
    return 'Log retention count must be between 1 and 100'
  }
  if (!inRange(draft.logRetentionDays, 1, 365)) {
    return 'Log retention days must be between 1 and 365'
  }
  if (draft.mcpEnabled && !inRange(draft.mcpPort, 1, 65535)) {
    return 'MCP server port must be between 1 and 65535'
  }

  return null
}

interface SettingsFooterProps {
  onClose: () => void
  onSave?: () => void
  isSaving?: boolean
}

function SettingsFooter({ onClose, onSave, isSaving }: SettingsFooterProps) {
  return (
    <Dialog.Footer
      px={3}
      py={2}
      bg='app.panel'
      borderTop='1px solid'
      borderColor='app.hover'
    >
      <Flex justify='flex-end' gap={2} width='100%'>
        <Button
          variant='ghost'
          size='xs'
          onClick={onClose}
          disabled={isSaving}
          _hover={{ bg: 'whiteAlpha.50' }}
          color='gray.400'
          height='28px'
          fontSize='xs'
        >
          Cancel
        </Button>
        <Button
          size='xs'
          onClick={onSave}
          loading={isSaving}
          loadingText='Saving...'
          disabled={!onSave}
          bg='blue.500'
          color='white'
          _hover={{ bg: 'blue.600' }}
          _active={{ bg: 'blue.700' }}
          height='28px'
          fontSize='xs'
        >
          Save Settings
        </Button>
      </Flex>
    </Dialog.Footer>
  )
}

interface SettingsFormProps {
  settings: Record<string, string>
  ssl: SslSettingsData
  log: LogSettingsData
  appMode: AppMode
  mcp: McpStatus
  onClose: () => void
}

function SettingsForm({
  settings,
  ssl,
  log,
  appMode,
  mcp,
  onClose,
}: SettingsFormProps) {
  const queryClient = useQueryClient()
  const [initial] = useState(() =>
    buildSettingsDraft({ settings, ssl, log, appMode, mcp }),
  )
  const [draft, setDraft] = useState(initial)
  const [isSaving, setIsSaving] = useState(false)

  const update = <K extends keyof SettingsDraft>(
    key: K,
    value: SettingsDraft[K],
  ) => setDraft(prev => ({ ...prev, [key]: value }))

  const saveSettings = async () => {
    const invalid = validationError(draft)

    if (invalid) {
      toaster.error({
        title: 'Invalid Input',
        description: invalid,
        duration: 3000,
      })

      return
    }

    const mcpPort = parseInt(draft.mcpPort, 10)
    const writes: [string, () => Promise<void>][] = [
      [
        'General',
        async () => {
          await invoke('update_disconnect_timeout', {
            minutes: parseInt(draft.disconnectTimeout, 10),
          })
          await invoke('update_network_monitor', {
            enabled: draft.networkMonitor,
          })
          await invoke('update_auto_update_enabled', {
            enabled: draft.autoUpdateEnabled,
          })
        },
      ],
      [
        'SSL',
        () =>
          invoke('set_ssl_settings', {
            sslEnabled: draft.sslEnabled,
            sslCertValidityDays: parseInt(draft.sslCertValidityDays, 10),
            sslAutoRegenerate: true,
            sslCaAutoInstall: true,
          }),
      ],
      [
        'Logs',
        () =>
          invoke('set_log_settings', {
            settings: {
              retention_count: parseInt(draft.logRetentionCount, 10),
              retention_days: parseInt(draft.logRetentionDays, 10),
            },
          }),
      ],
      [
        'Window',
        async () => {
          if (draft.appMode !== initial.appMode) {
            await invoke('set_app_mode_cmd', { mode: draft.appMode })
          }
        },
      ],
      [
        'MCP Server',
        async () => {
          if (!Number.isNaN(mcpPort) && draft.mcpPort !== initial.mcpPort) {
            await invoke('update_mcp_server_port', { port: mcpPort })
          }
          if (draft.mcpEnabled !== initial.mcpEnabled) {
            await invoke('update_mcp_server_enabled', {
              enabled: draft.mcpEnabled,
            })
          }
        },
      ],
    ]

    setIsSaving(true)
    const failures: string[] = []

    for (const [section, write] of writes) {
      try {
        await write()
      } catch (error) {
        failures.push(`${section}: ${errorMessage(error)}`)
      }
    }

    for (const query of SETTINGS_QUERIES) {
      queryClient.invalidateQueries({ queryKey: query.queryKey })
    }
    setIsSaving(false)

    if (failures.length > 0) {
      toaster.error({
        title: 'Some settings were not saved',
        description: failures.join('\n'),
        duration: 6000,
      })

      return
    }

    const sslJustEnabled = !initial.sslEnabled && draft.sslEnabled

    toaster.success(
      sslJustEnabled
        ? {
            title: 'SSL/HTTPS Enabled Successfully',
            description:
              'SSL certificates have been generated and installed. You may need to restart your browser for SSL connections to work properly.',
            duration: 8000,
          }
        : {
            title: 'Settings Saved',
            description: 'All settings have been saved successfully',
            duration: 3000,
          },
    )
    onClose()
  }

  return (
    <>
      <Dialog.Body
        p={3}
        overflowY='auto'
        css={{
          '&::-webkit-scrollbar': { width: '6px' },
          '&::-webkit-scrollbar-track': { background: 'transparent' },
          '&::-webkit-scrollbar-thumb': {
            background: 'var(--chakra-colors-app-divider)',
            borderRadius: '3px',
          },
        }}
      >
        <Box display='grid' gridTemplateColumns='1fr 1fr' gap={2.5}>
          <NetworkSettings
            disconnectTimeout={draft.disconnectTimeout}
            onDisconnectTimeoutChange={value =>
              update('disconnectTimeout', value)
            }
            networkMonitorEnabled={draft.networkMonitor}
            onNetworkMonitorEnabledChange={value =>
              update('networkMonitor', value)
            }
            networkMonitorRunning={settings.network_monitor_status === 'true'}
          />
          <SslSettings
            sslEnabled={draft.sslEnabled}
            onSslEnabledChange={value => update('sslEnabled', value)}
            sslCertValidityDays={draft.sslCertValidityDays}
            onSslCertValidityDaysChange={value =>
              update('sslCertValidityDays', value)
            }
          />
          <UpdateSettings
            autoUpdateEnabled={draft.autoUpdateEnabled}
            onAutoUpdateEnabledChange={value =>
              update('autoUpdateEnabled', value)
            }
            lastUpdateCheck={settings.last_update_check}
          />
          <LogSettings
            retentionCount={draft.logRetentionCount}
            onRetentionCountChange={value => update('logRetentionCount', value)}
            retentionDays={draft.logRetentionDays}
            onRetentionDaysChange={value => update('logRetentionDays', value)}
          />
          <WindowSettings
            appMode={draft.appMode}
            onAppModeChange={value => update('appMode', value)}
          />
          <McpServerSettings
            enabled={draft.mcpEnabled}
            port={draft.mcpPort}
            running={mcp.running === 'true'}
            onEnabledChange={value => update('mcpEnabled', value)}
            onPortChange={value => update('mcpPort', value)}
          />
        </Box>
      </Dialog.Body>
      <SettingsFooter
        onClose={onClose}
        onSave={saveSettings}
        isSaving={isSaving}
      />
    </>
  )
}

export default function SettingsModal({ onClose }: { onClose: () => void }) {
  const [settings, ssl, log, appMode, mcp] = useQueries({
    queries: SETTINGS_QUERIES,
  })
  const loadError = [settings, ssl, log, appMode, mcp].find(
    query => query.error,
  )?.error

  return (
    <AppDialog
      title='Settings'
      onClose={onClose}
      maxWidth='600px'
      height='92vh'
    >
      {settings.data && ssl.data && log.data && appMode.data && mcp.data ? (
        <SettingsForm
          settings={settings.data}
          ssl={ssl.data}
          log={log.data}
          appMode={appMode.data}
          mcp={mcp.data}
          onClose={onClose}
        />
      ) : (
        <>
          <Dialog.Body p={3}>
            <Stack align='center' justify='center' height='100%'>
              {loadError ? (
                <Text fontSize='xs' color='red.300'>
                  Failed to load settings: {errorMessage(loadError)}
                </Text>
              ) : (
                <Spinner size='sm' color='whiteAlpha.600' />
              )}
            </Stack>
          </Dialog.Body>
          <SettingsFooter onClose={onClose} />
        </>
      )}
    </AppDialog>
  )
}
