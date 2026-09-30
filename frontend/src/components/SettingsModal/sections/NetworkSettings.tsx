import type { ChangeEvent } from 'react'

import { Input } from '@chakra-ui/react'

import { Checkbox } from '@/components/ui/checkbox'

import { compactInputProps, SettingCard, SettingRow } from '../SettingCard'

interface NetworkSettingsProps {
  disconnectTimeout: string
  onDisconnectTimeoutChange: (value: string) => void
  networkMonitorEnabled: boolean
  onNetworkMonitorEnabledChange: (enabled: boolean) => void
  networkMonitorRunning: boolean
}

export function NetworkSettings({
  disconnectTimeout,
  onDisconnectTimeoutChange,
  networkMonitorEnabled,
  onNetworkMonitorEnabledChange,
  networkMonitorRunning,
}: NetworkSettingsProps) {
  const handleTimeoutChange = (e: ChangeEvent<HTMLInputElement>) => {
    const value = e.target.value

    if (value === '' || /^\d+$/.test(value)) {
      onDisconnectTimeoutChange(value)
    }
  }

  return (
    <>
      <SettingCard
        title='Auto-disconnect Timeout'
        description='Disconnect port forwards after specified time (min). Set to 0 to disable.'
      >
        <SettingRow label='Minutes:'>
          <Input
            {...compactInputProps}
            value={disconnectTimeout}
            onChange={handleTimeoutChange}
            placeholder='0'
            width='45px'
          />
        </SettingRow>
      </SettingCard>

      <SettingCard
        title='Network Monitor'
        description='Monitor connectivity and reconnect port forwards when network is restored.'
        statusColor={networkMonitorRunning ? 'success.fg' : 'fg.subtle'}
        statusTitle={networkMonitorRunning ? 'Running' : 'Stopped'}
      >
        <SettingRow label='Enabled:'>
          <Checkbox
            checked={networkMonitorEnabled}
            onCheckedChange={e =>
              onNetworkMonitorEnabledChange(e.checked === true)
            }
            size='sm'
          />
        </SettingRow>
      </SettingCard>
    </>
  )
}
