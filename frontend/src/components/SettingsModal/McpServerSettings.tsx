import { Server } from 'lucide-react'

import { Input } from '@chakra-ui/react'

import { Checkbox } from '@/components/ui/checkbox'

import {
  compactInputProps,
  isDigitsUpTo,
  SettingCard,
  SettingRow,
} from './SettingCard'

interface McpServerSettingsProps {
  enabled: boolean
  port: string
  running: boolean
  onEnabledChange: (enabled: boolean) => void
  onPortChange: (port: string) => void
}

export function McpServerSettings({
  enabled,
  port,
  running,
  onEnabledChange,
  onPortChange,
}: McpServerSettingsProps) {
  return (
    <>
      <SettingCard
        title='MCP Server'
        description='Enable MCP server for AI assistants to manage port forwards via Model Context Protocol.'
        icon={Server}
        iconColor='accent.secondary'
        statusColor={running ? 'success.fg' : 'fg.subtle'}
        statusTitle={running ? 'Running' : 'Stopped'}
      >
        <SettingRow label='Enabled:'>
          <Checkbox
            checked={enabled}
            onCheckedChange={e => onEnabledChange(e.checked === true)}
            size='sm'
          />
        </SettingRow>
      </SettingCard>

      <SettingCard
        title='MCP Server Port'
        description={
          running
            ? `Running at http://127.0.0.1:${port}`
            : 'Server endpoint port'
        }
        opacity={enabled ? 1 : 0.5}
      >
        <SettingRow label='Port:'>
          <Input
            {...compactInputProps}
            value={port}
            onChange={e => {
              if (isDigitsUpTo(e.target.value, 65535)) {
                onPortChange(e.target.value)
              }
            }}
            placeholder='3000'
            width='55px'
            disabled={!enabled}
          />
        </SettingRow>
      </SettingCard>
    </>
  )
}
