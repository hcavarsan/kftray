import Select from 'react-select'

import { Grid, Text } from '@chakra-ui/react'

import { Checkbox } from '@/components/ui/checkbox'
import { selectStyles } from '@/components/ui/select-styles'

import { Field, TextField } from './Field'
import type { ConfigDraft, StringOption } from './types'

const protocolOptions: StringOption[] = [
  { value: 'tcp', label: 'TCP' },
  { value: 'udp', label: 'UDP' },
]

interface ProxyFieldsProps {
  draft: ConfigDraft
  errors: Record<string, string>
  onUpdate: (update: Partial<ConfigDraft>) => void
}

export function ProxyFields({ draft, errors, onUpdate }: ProxyFieldsProps) {
  return (
    <>
      <Grid templateColumns='repeat(2, 1fr)' gap={3}>
        <TextField
          error={errors.remote_address}
          label='Remote Address *'
          name='remote_address'
          onChange={remote_address => onUpdate({ remote_address })}
          value={draft.remote_address ?? ''}
        />
        <Field error={errors.protocol} label='Protocol *'>
          <Select<StringOption>
            onChange={option =>
              onUpdate({ protocol: option?.value === 'udp' ? 'udp' : 'tcp' })
            }
            options={protocolOptions}
            styles={selectStyles<StringOption>()}
            value={
              protocolOptions.find(option => option.value === draft.protocol) ??
              null
            }
          />
        </Field>
      </Grid>
      <Grid templateColumns='repeat(2, 1fr)' gap={3}>
        <TextField
          error={errors.remote_port}
          label='Target Port *'
          name='remote_port'
          onChange={remote_port => onUpdate({ remote_port })}
          type='number'
          value={draft.remote_port ?? ''}
        />
        <TextField
          error={errors.local_port}
          label='Local Port'
          name='local_port'
          onChange={local_port => onUpdate({ local_port })}
          type='number'
          value={draft.local_port ?? ''}
        />
      </Grid>
      <TextField
        disabled={draft.auto_loopback_address}
        label='Local Address (Optional)'
        name='local_address'
        onChange={local_address => onUpdate({ local_address })}
        placeholder={
          draft.auto_loopback_address ? '127.0.0.x' : 'e.g., 127.0.0.1'
        }
        value={
          draft.auto_loopback_address
            ? ''
            : (draft.local_address ?? '127.0.0.1')
        }
      />
      <Checkbox
        checked={draft.auto_loopback_address ?? false}
        onCheckedChange={event =>
          onUpdate({
            auto_loopback_address: event.checked === true,
            local_address: event.checked === true ? '' : '127.0.0.1',
          })
        }
        size='xs'
      >
        <Text fontSize='xs' color='gray.400'>
          Auto select address
        </Text>
      </Checkbox>
    </>
  )
}
