import CreatableSelect from 'react-select/creatable'

import { Grid } from '@chakra-ui/react'

import { selectStyles } from '@/components/ui/select-styles'

import { Field, LocalAddressField, ProtocolField, TextField } from './Field'
import type { ConfigDraft, PortOption, StringOption } from './types'

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
          label={isPod ? 'Pod Label' : 'Service'}
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
        <ProtocolField
          draft={draft}
          error={errors.protocol}
          onUpdate={onUpdate}
        />
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
        <LocalAddressField draft={draft} onUpdate={onUpdate} />
      </Grid>
    </>
  )
}
