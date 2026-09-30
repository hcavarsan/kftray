import type { ChangeEvent } from 'react'
import { Shield } from 'lucide-react'

import { Input } from '@chakra-ui/react'

import { Checkbox } from '@/components/ui/checkbox'

import {
  compactInputProps,
  isDigitsUpTo,
  SettingCard,
  SettingRow,
} from '../SettingCard'

interface SslSettingsProps {
  sslEnabled: boolean
  onSslEnabledChange: (enabled: boolean) => void
  sslCertValidityDays: string
  onSslCertValidityDaysChange: (value: string) => void
}

export function SslSettings({
  sslEnabled,
  onSslEnabledChange,
  sslCertValidityDays,
  onSslCertValidityDaysChange,
}: SslSettingsProps) {
  const handleCertValidityChange = (e: ChangeEvent<HTMLInputElement>) => {
    const value = e.target.value

    if (isDigitsUpTo(value, 3650)) {
      onSslCertValidityDaysChange(value)
    }
  }

  return (
    <>
      <SettingCard
        title='SSL/HTTPS'
        description='Enable HTTPS for port forwards with domain aliases. Creates SSL certificates automatically.'
        icon={Shield}
        iconColor='blue.400'
        statusColor={sslEnabled ? 'green.400' : 'gray.500'}
        statusTitle={sslEnabled ? 'Enabled' : 'Disabled'}
      >
        <SettingRow label='Enabled:'>
          <Checkbox
            checked={sslEnabled}
            onCheckedChange={e => onSslEnabledChange(e.checked === true)}
            size='sm'
          />
        </SettingRow>
      </SettingCard>

      <SettingCard
        title='Certificate Validity'
        description='Configure SSL certificate validity period. Certificates will auto-regenerate and CA will be auto-installed.'
        opacity={sslEnabled ? 1 : 0.5}
      >
        <SettingRow label='Validity (days):'>
          <Input
            {...compactInputProps}
            value={sslCertValidityDays}
            onChange={handleCertValidityChange}
            placeholder='365'
            width='55px'
            disabled={!sslEnabled}
          />
        </SettingRow>
      </SettingCard>
    </>
  )
}
