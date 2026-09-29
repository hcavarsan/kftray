import Select from 'react-select'
import CreatableSelect from 'react-select/creatable'

import { Grid, Stack, Text } from '@chakra-ui/react'

import { Checkbox } from '@/components/ui/checkbox'
import { selectStyles } from '@/components/ui/select-styles'

import { Field, TextField } from './Field'
import type { ConfigDraft, PortOption, StringOption } from './types'

const protocolOptions: StringOption[] = [
  { value: 'tcp', label: 'TCP' },
  { value: 'udp', label: 'UDP' },
]

interface KubeTargetFieldsProps {
  draft: ConfigDraft
  errors: Record<string, string>
  isLoading: boolean
  noOptionsMessage: string
  onUpdate: (update: Partial<ConfigDraft>) => void
  options: StringOption[]
  portError?: string
  portOptions: PortOption[]
  portsLoading: boolean
  portsMessage: string
}

export function KubeTargetFields({
  draft,
  errors,
  isLoading,
  noOptionsMessage,
  onUpdate,
  options,
  portError,
  portOptions,
  portsLoading,
  portsMessage,
}: KubeTargetFieldsProps) {
  const isPod = draft.workload_type === 'pod'
  const targetValue = isPod ? draft.target : draft.service
  return (
    <>
      <Grid templateColumns='repeat(2, 1fr)' gap={3}>
        <Field
          error={isPod ? errors.target : errors.service}
          label={isPod ? 'Pod Label *' : 'Service *'}
        >
          <CreatableSelect<StringOption>
            formatCreateLabel={value => `Use "${value}"`}
            isLoading={isLoading}
            noOptionsMessage={() => noOptionsMessage}
            onChange={option =>
              onUpdate(
                isPod
                  ? { target: option?.value ?? '' }
                  : { service: option?.value ?? '' },
              )
            }
            options={options}
            styles={selectStyles<StringOption>()}
            value={
              targetValue ? { label: targetValue, value: targetValue } : null
            }
          />
        </Field>
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
        <Field error={portError ?? errors.remote_port} label='Target Port *'>
          <CreatableSelect<PortOption>
            formatCreateLabel={value => `Use port ${value}`}
            isDisabled={!draft.context || !draft.namespace}
            isLoading={portsLoading}
            noOptionsMessage={() => portsMessage}
            onChange={option => onUpdate({ remote_port: option?.value ?? '' })}
            options={portOptions}
            placeholder='Select or type port'
            styles={selectStyles<PortOption>()}
            value={
              draft.remote_port
                ? { label: draft.remote_port, value: draft.remote_port }
                : null
            }
          />
        </Field>
        <TextField
          error={errors.local_port}
          label='Local Port'
          name='local_port'
          onChange={local_port => onUpdate({ local_port })}
          type='number'
          value={draft.local_port ?? ''}
        />
      </Grid>
      <Stack gap={1.5}>
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
      </Stack>
    </>
  )
}
