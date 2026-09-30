import { Info } from 'lucide-react'

import { Grid, Stack, Text } from '@chakra-ui/react'

import { Checkbox } from '@/components/ui/checkbox'
import { selectStyles } from '@/components/ui/select-styles'
import { Tooltip } from '@/components/ui/tooltip'

import { Field, FieldCreatableSelect, FieldSelect, TextField } from './Field'
import type { ConfigDraft, StringOption } from './types'

const workloadTypeOptions: StringOption[] = [
  { value: 'service', label: 'Service' },
  { value: 'pod', label: 'Pod' },
  { value: 'proxy', label: 'Proxy' },
  { value: 'expose', label: 'Expose' },
]

function ErrorText({ error, label }: { error: unknown; label: string }) {
  return error ? (
    <Text color='danger.fg' fontSize='xs'>
      {label}
    </Text>
  ) : null
}

interface NamedResourceQuery {
  data?: { name: string }[]
  error: unknown
  isLoading: boolean
}

interface CommonFieldsProps {
  contextQuery: NamedResourceQuery
  draft: ConfigDraft
  errors: Record<string, string>
  namespaceQuery: NamedResourceQuery
  onContextFocusChange: (focused: boolean) => void
  onUpdate: (update: Partial<ConfigDraft>) => void
}

export function CommonFields({
  contextQuery,
  draft,
  errors,
  namespaceQuery,
  onContextFocusChange,
  onUpdate,
}: CommonFieldsProps) {
  const workloadType = draft.workload_type

  return (
    <>
      <Grid templateColumns='repeat(2, 1fr)' gap={3}>
        <Stack gap={1.5}>
          <TextField
            error={workloadType === 'expose' ? errors.alias : undefined}
            hint={
              workloadType === 'expose' ? (
                <Tooltip
                  content={
                    draft.exposure_type === 'public'
                      ? 'Full domain for public access (e.g., myapp.example.com). The Kubernetes service will be named using the first part before the dot (e.g., "myapp").'
                      : `Service name in cluster (accessible as ${draft.alias || 'name'}.${draft.namespace || 'namespace'}.svc.cluster.local)`
                  }
                >
                  <span
                    style={{ display: 'inline-flex', alignItems: 'center' }}
                  >
                    <Info size={10} color='var(--chakra-colors-fg-subtle)' />
                  </span>
                </Tooltip>
              ) : undefined
            }
            label={workloadType === 'expose' ? 'Domain *' : 'Alias'}
            name='alias'
            onChange={alias => onUpdate({ alias })}
            placeholder={
              workloadType === 'expose'
                ? draft.exposure_type === 'public'
                  ? 'myapp.example.com'
                  : 'my-service'
                : ''
            }
            value={draft.alias ?? ''}
          />
          {workloadType !== 'expose' && (
            <Checkbox
              checked={draft.domain_enabled ?? false}
              onCheckedChange={event =>
                onUpdate({ domain_enabled: event.checked === true })
              }
              size='xs'
            >
              <Text color='fg.muted' fontSize='xs'>
                Enable alias as domain
              </Text>
            </Checkbox>
          )}
        </Stack>
        <Field error={errors.context} label='Context *'>
          <FieldSelect<StringOption>
            isLoading={contextQuery.isLoading}
            onBlur={() => onContextFocusChange(false)}
            onChange={option => onUpdate({ context: option?.value ?? '' })}
            onFocus={() => onContextFocusChange(true)}
            options={(contextQuery.data ?? []).map(({ name }) => ({
              label: name,
              value: name,
            }))}
            styles={selectStyles<StringOption>()}
            value={
              draft.context
                ? { label: draft.context, value: draft.context }
                : null
            }
          />
          <ErrorText
            error={contextQuery.error}
            label='Error fetching contexts'
          />
        </Field>
      </Grid>
      <Grid templateColumns='repeat(2, 1fr)' gap={3}>
        <Field error={errors.workload_type} label='Workload Type'>
          <FieldSelect<StringOption>
            onChange={option =>
              onUpdate({
                exposure_type:
                  option?.value === 'expose'
                    ? (draft.exposure_type ?? 'cluster')
                    : draft.exposure_type,
                protocol: option?.value === 'expose' ? 'tcp' : draft.protocol,
                workload_type:
                  option?.value === 'pod'
                    ? 'pod'
                    : option?.value === 'proxy'
                      ? 'proxy'
                      : option?.value === 'expose'
                        ? 'expose'
                        : option?.value === 'service'
                          ? 'service'
                          : undefined,
              })
            }
            options={workloadTypeOptions}
            styles={selectStyles<StringOption>()}
            value={
              workloadTypeOptions.find(
                option => option.value === workloadType,
              ) ?? null
            }
          />
        </Field>
        <Field error={errors.namespace} label='Namespace *'>
          <FieldCreatableSelect<StringOption>
            formatCreateLabel={value => `Use "${value}"`}
            isLoading={namespaceQuery.isLoading}
            noOptionsMessage={() =>
              namespaceQuery.error
                ? 'Type namespace name manually'
                : 'No namespaces found'
            }
            onChange={option => onUpdate({ namespace: option?.value ?? '' })}
            options={(namespaceQuery.data ?? []).map(({ name }) => ({
              label: name,
              value: name,
            }))}
            styles={selectStyles<StringOption>()}
            value={
              draft.namespace
                ? { label: draft.namespace, value: draft.namespace }
                : null
            }
          />
          <ErrorText
            error={namespaceQuery.error}
            label='Error fetching namespaces'
          />
        </Field>
      </Grid>
    </>
  )
}
