import { Grid } from '@chakra-ui/react'

import { LocalAddressField, ProtocolField, TextField } from './Field'
import type { ConfigDraft } from './types'

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
          label='Remote Address'
          name='remote_address'
          onChange={remote_address => onUpdate({ remote_address })}
          value={draft.remote_address ?? ''}
        />
        <ProtocolField
          draft={draft}
          error={errors.protocol}
          onUpdate={onUpdate}
        />
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
      <Grid templateColumns='repeat(2, 1fr)' gap={3}>
        <LocalAddressField draft={draft} onUpdate={onUpdate} />
      </Grid>
    </>
  )
}
