import { Checkbox } from '@/components/ui/checkbox'

import { SettingCard, SettingRow } from '../SettingCard'

interface PerformanceSettingsProps {
  enabled: boolean
  onEnabledChange: (enabled: boolean) => void
}

export function PerformanceSettings({
  enabled,
  onEnabledChange,
}: PerformanceSettingsProps) {
  return (
    <SettingCard
      title='Performance Data'
      description='Send how long port forwards take to start and stop.'
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
