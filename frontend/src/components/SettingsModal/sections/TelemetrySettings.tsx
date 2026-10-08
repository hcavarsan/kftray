import { Checkbox } from '@/components/ui/checkbox'

import { SettingCard, SettingRow } from '../SettingCard'

interface TelemetrySettingsProps {
  enabled: boolean
  onEnabledChange: (enabled: boolean) => void
}

export function TelemetrySettings({
  enabled,
  onEnabledChange,
}: TelemetrySettingsProps) {
  return (
    <SettingCard
      title='Crash Reports'
      description='Send a report when kftray crashes or hits an unexpected error. Reports never include cluster names, namespaces, services, aliases or kubeconfig files.'
    >
      <SettingRow label='Enabled:'>
        <Checkbox
          checked={enabled}
          onCheckedChange={e => onEnabledChange(e.checked === true)}
          size='sm'
        />
      </SettingRow>
    </SettingCard>
  )
}
