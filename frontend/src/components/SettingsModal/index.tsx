import { useState } from 'react'

import { Box, Spinner, Stack, Text } from '@chakra-ui/react'
import { useQueries } from '@tanstack/react-query'

import type { LogSettings as LogSettingsData } from '@/components/LogViewer'
import { Button } from '@/components/ui/button'
import {
  AppDialog,
  AppDialogBody,
  AppDialogFooter,
  DialogCancelButton,
} from '@/components/ui/dialog'
import { errorMessage } from '@/lib/errors'

import { McpServerSettings } from './McpServerSettings'
import { SETTINGS_QUERIES } from './queries'
import { LogSettings } from './sections/LogSettings'
import { NetworkSettings } from './sections/NetworkSettings'
import { SslSettings } from './sections/SslSettings'
import { TelemetrySettings } from './sections/TelemetrySettings'
import { UpdateSettings } from './sections/UpdateSettings'
import {
  type AppMode,
  buildSettingsDraft,
  type McpStatus,
  type SettingsDraft,
  type SslSettingsData,
} from './types'
import { type LoadedSections, useSettingsSave } from './useSettingsSave'
import { WindowSettings } from './WindowSettings'

interface SettingsFooterProps {
  onClose: () => void
  onSave?: () => void
  isSaving?: boolean
}

function SettingsFooter({ onClose, onSave, isSaving }: SettingsFooterProps) {
  return (
    <AppDialogFooter>
      <DialogCancelButton onClick={onClose} disabled={isSaving} />
      <Button
        size='xs'
        onClick={onSave}
        loading={isSaving}
        loadingText='Saving...'
        disabled={!onSave}
        bg='accent.solid'
        color='fg'
        _hover={{ bg: 'accent.solidHover' }}
        _active={{ bg: 'accent.solidActive' }}
        height='28px'
        fontSize='xs'
      >
        Save Settings
      </Button>
    </AppDialogFooter>
  )
}

interface SettingsFormProps {
  settings: Record<string, string>
  ssl: SslSettingsData | undefined
  log: LogSettingsData | undefined
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
  const [initial] = useState(() =>
    buildSettingsDraft({ settings, ssl, log, appMode, mcp }),
  )
  const [loaded] = useState<LoadedSections>(() => ({
    ssl: ssl !== undefined,
    log: log !== undefined,
  }))
  const [draft, setDraft] = useState(initial)
  const { save, isSaving } = useSettingsSave({
    draft,
    initial,
    loaded,
    onClose,
  })

  const update = <K extends keyof SettingsDraft>(
    key: K,
    value: SettingsDraft[K],
  ) => setDraft(prev => ({ ...prev, [key]: value }))

  return (
    <>
      <AppDialogBody>
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
            disabled={!loaded.ssl}
          />
          <UpdateSettings
            autoUpdateEnabled={draft.autoUpdateEnabled}
            onAutoUpdateEnabledChange={value =>
              update('autoUpdateEnabled', value)
            }
            lastUpdateCheck={settings.last_update_check}
          />
          <TelemetrySettings
            enabled={draft.telemetryEnabled}
            onEnabledChange={value => update('telemetryEnabled', value)}
          />
          <LogSettings
            retentionCount={draft.logRetentionCount}
            onRetentionCountChange={value => update('logRetentionCount', value)}
            retentionDays={draft.logRetentionDays}
            onRetentionDaysChange={value => update('logRetentionDays', value)}
            disabled={!loaded.log}
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
      </AppDialogBody>
      <SettingsFooter onClose={onClose} onSave={save} isSaving={isSaving} />
    </>
  )
}

export function SettingsModal({ onClose }: { onClose: () => void }) {
  const [settings, ssl, log, appMode, mcp] = useQueries({
    queries: SETTINGS_QUERIES,
  })
  const loadError = [settings, appMode, mcp].find(query => query.error)?.error
  const sslSettled = ssl.status !== 'pending'
  const logSettled = log.status !== 'pending'

  return (
    <AppDialog
      title='Settings'
      onClose={onClose}
      maxWidth='600px'
      height='92vh'
    >
      {settings.data && appMode.data && mcp.data && sslSettled && logSettled ? (
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
          <AppDialogBody>
            <Stack align='center' justify='center' height='100%'>
              {loadError ? (
                <Text fontSize='xs' color='danger.fg'>
                  Failed to load settings: {errorMessage(loadError)}
                </Text>
              ) : (
                <Spinner size='sm' color='fg.subtle' />
              )}
            </Stack>
          </AppDialogBody>
          <SettingsFooter onClose={onClose} />
        </>
      )}
    </AppDialog>
  )
}
