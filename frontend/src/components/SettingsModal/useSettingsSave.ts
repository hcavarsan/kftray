import { useState } from 'react'

import { useMutation, useQueryClient } from '@tanstack/react-query'
import { invoke } from '@tauri-apps/api/core'

import { toaster } from '@/components/ui/toaster'
import { errorMessage } from '@/lib/errors'

import { SETTINGS_QUERIES } from './queries'
import type { SettingsDraft } from './types'

const MAX_DISCONNECT_TIMEOUT = 4294967295

const inRange = (value: string, min: number, max: number) => {
  const parsed = parseInt(value, 10)

  return !Number.isNaN(parsed) && parsed >= min && parsed <= max
}

type SettingsSection = 'General' | 'SSL' | 'Logs' | 'Window' | 'MCP Server'

interface ToastCopy {
  title: string
  description: string
  duration: number
}

const SECTION_FAILURES: Record<SettingsSection, (error: unknown) => ToastCopy> =
  {
    General: () => ({
      title: 'Error',
      description: 'Failed to save settings',
      duration: 3000,
    }),
    SSL: () => ({
      title: 'SSL Settings Error',
      description: 'Failed to save SSL settings, but other settings were saved',
      duration: 4000,
    }),
    Logs: () => ({
      title: 'Log Settings Error',
      description: 'Failed to save log settings, but other settings were saved',
      duration: 4000,
    }),
    Window: error => ({
      title: 'Window Settings Error',
      description: `Failed to apply window settings: ${errorMessage(error)}`,
      duration: 4000,
    }),
    'MCP Server': error => ({
      title: 'MCP Server Error',
      description: `Failed to apply MCP server settings: ${errorMessage(error)}`,
      duration: 4000,
    }),
  }

function validationError(
  draft: SettingsDraft,
): Pick<ToastCopy, 'title' | 'description'> | null {
  if (!inRange(draft.disconnectTimeout, 0, MAX_DISCONNECT_TIMEOUT)) {
    return {
      title: 'Invalid Input',
      description: `Please enter a valid number (0 to ${MAX_DISCONNECT_TIMEOUT}) for timeout`,
    }
  }
  if (!inRange(draft.sslCertValidityDays, 1, 3650)) {
    return {
      title: 'Invalid Input',
      description: 'Certificate validity must be between 1 and 3650 days',
    }
  }
  if (!inRange(draft.logRetentionCount, 1, 100)) {
    return {
      title: 'Invalid Input',
      description: 'Log retention count must be between 1 and 100',
    }
  }
  if (!inRange(draft.logRetentionDays, 1, 365)) {
    return {
      title: 'Invalid Input',
      description: 'Log retention days must be between 1 and 365',
    }
  }
  if (draft.mcpEnabled && !inRange(draft.mcpPort, 1, 65535)) {
    return {
      title: 'Invalid Port',
      description: 'MCP server port must be between 1 and 65535',
    }
  }

  return null
}

export interface LoadedSections {
  ssl: boolean
  log: boolean
}

interface SectionPlan {
  section: SettingsSection
  keys: (keyof SettingsDraft)[]
  write: () => Promise<void>
  skip?: boolean
}

interface SectionFailure {
  section: SettingsSection
  error: unknown
}

interface UseSettingsSaveOptions {
  draft: SettingsDraft
  initial: SettingsDraft
  loaded: LoadedSections
  onClose: () => void
}

function buildPlans(
  draft: SettingsDraft,
  baseline: SettingsDraft,
  loaded: LoadedSections,
): SectionPlan[] {
  const mcpPort = parseInt(draft.mcpPort, 10)

  return [
    {
      section: 'General',
      keys: ['disconnectTimeout', 'networkMonitor', 'autoUpdateEnabled'],
      write: async () => {
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
    },
    {
      section: 'SSL',
      skip: !loaded.ssl,
      keys: ['sslEnabled', 'sslCertValidityDays'],
      write: () =>
        invoke('set_ssl_settings', {
          sslEnabled: draft.sslEnabled,
          sslCertValidityDays: parseInt(draft.sslCertValidityDays, 10),
          sslAutoRegenerate: true,
          sslCaAutoInstall: true,
        }),
    },
    {
      section: 'Logs',
      skip: !loaded.log,
      keys: ['logRetentionCount', 'logRetentionDays'],
      write: () =>
        invoke('set_log_settings', {
          settings: {
            retention_count: parseInt(draft.logRetentionCount, 10),
            retention_days: parseInt(draft.logRetentionDays, 10),
          },
        }),
    },
    {
      section: 'Window',
      keys: ['appMode'],
      write: () => invoke('set_app_mode_cmd', { mode: draft.appMode }),
    },
    {
      section: 'MCP Server',
      keys: Number.isNaN(mcpPort) ? ['mcpEnabled'] : ['mcpPort', 'mcpEnabled'],
      write: async () => {
        if (!Number.isNaN(mcpPort) && draft.mcpPort !== baseline.mcpPort) {
          await invoke('update_mcp_server_port', { port: mcpPort })
        }
        if (draft.mcpEnabled !== baseline.mcpEnabled) {
          await invoke('update_mcp_server_enabled', {
            enabled: draft.mcpEnabled,
          })
        }
      },
    },
  ]
}

export function useSettingsSave({
  draft,
  initial,
  loaded,
  onClose,
}: UseSettingsSaveOptions) {
  const queryClient = useQueryClient()
  const [baseline, setBaseline] = useState(initial)

  const saveMutation = useMutation({
    mutationFn: async () => {
      const failures: SectionFailure[] = []

      for (const { section, keys, write, skip } of buildPlans(
        draft,
        baseline,
        loaded,
      )) {
        if (skip || keys.every(key => draft[key] === baseline[key])) {
          continue
        }

        try {
          await write()
          setBaseline(prev => ({
            ...prev,
            ...Object.fromEntries(keys.map(key => [key, draft[key]])),
          }))
        } catch (error) {
          failures.push({ section, error })
        }
      }

      for (const query of SETTINGS_QUERIES) {
        queryClient.invalidateQueries({ queryKey: query.queryKey })
      }

      return failures
    },
    onSuccess: failures => {
      if (failures.length > 0) {
        toaster.error(
          failures.length === 1
            ? SECTION_FAILURES[failures[0].section](failures[0].error)
            : {
                title: 'Some settings were not saved',
                description: failures
                  .map(
                    ({ section, error }) =>
                      `${section}: ${errorMessage(error)}`,
                  )
                  .join('\n'),
                duration: 6000,
              },
        )

        return
      }

      const sslJustEnabled = !baseline.sslEnabled && draft.sslEnabled

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
    },
  })

  const save = () => {
    const invalid = validationError(draft)

    if (invalid) {
      toaster.error({
        ...invalid,
        duration: 3000,
      })

      return
    }

    saveMutation.mutate()
  }

  return { save, isSaving: saveMutation.isPending }
}
