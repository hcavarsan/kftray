import { Info } from 'lucide-react'
import Select from 'react-select'

import { Flex, Stack, Text } from '@chakra-ui/react'

import { Checkbox } from '@/components/ui/checkbox'
import { selectStyles } from '@/components/ui/select-styles'
import { Tooltip } from '@/components/ui/tooltip'

import { Field, TextField } from './Field'
import type { ConfigDraft, StringOption } from './types'

const exposureOptions: StringOption[] = [
  { value: 'cluster', label: 'Cluster Only (Internal)' },
  { value: 'public', label: 'Public (Internet)' },
]
const issuerOptions: StringOption[] = [
  { value: 'ClusterIssuer', label: 'ClusterIssuer' },
  { value: 'Issuer', label: 'Issuer' },
]

interface ExposeFieldsProps {
  draft: ConfigDraft
  errors: Record<string, string>
  onUpdate: (update: Partial<ConfigDraft>) => void
}

const Hint = ({ content }: { content: string }) => (
  <Tooltip content={content}>
    <span style={{ alignItems: 'center', display: 'inline-flex' }}>
      <Info color='var(--chakra-colors-fg-subtle)' size={10} />
    </span>
  </Tooltip>
)

export function ExposeFields({ draft, errors, onUpdate }: ExposeFieldsProps) {
  const isPublic = draft.exposure_type === 'public'
  return (
    <Stack gap={3}>
      <Field
        error={errors.exposure_type}
        hint={
          <Hint content='Cluster Only: Accessible within cluster via DNS. Public: Exposed to internet via Ingress.' />
        }
        label='Exposure Type *'
      >
        <Select<StringOption>
          onChange={option =>
            onUpdate({
              exposure_type: option?.value === 'public' ? 'public' : 'cluster',
            })
          }
          options={exposureOptions}
          styles={selectStyles<StringOption>()}
          value={
            exposureOptions.find(
              option => option.value === (draft.exposure_type ?? 'cluster'),
            ) ?? null
          }
        />
      </Field>
      <TextField
        error={errors.local_port}
        hint={<Hint content='Port of your local development server' />}
        label='Local Port *'
        name='local_port'
        onChange={local_port => onUpdate({ local_port })}
        placeholder='3000'
        type='number'
        value={draft.local_port ?? ''}
      />
      <TextField
        hint={
          <Hint content='Address where your local service is running (default: 127.0.0.1)' />
        }
        label='Local Address'
        name='local_address'
        onChange={local_address => onUpdate({ local_address })}
        placeholder='127.0.0.1'
        value={draft.local_address ?? '127.0.0.1'}
      />
      {isPublic && (
        <>
          <Flex align='center' gap={1}>
            <Checkbox
              checked={draft.cert_manager_enabled ?? false}
              onCheckedChange={event => {
                const cert_manager_enabled = event.checked === true
                onUpdate({
                  cert_manager_enabled,
                  domain_enabled: cert_manager_enabled,
                  cert_issuer_kind: cert_manager_enabled
                    ? (draft.cert_issuer_kind ?? 'ClusterIssuer')
                    : draft.cert_issuer_kind,
                })
              }}
              size='xs'
            >
              <Text fontSize='xs' color='fg.muted'>
                Enable HTTPS (cert-manager)
              </Text>
            </Checkbox>
            <Hint content='Automatically provision TLS certificate using cert-manager' />
          </Flex>
          {draft.cert_manager_enabled && (
            <>
              <Field error={errors.cert_issuer_kind} label='Issuer Kind *'>
                <Select<StringOption>
                  onChange={option =>
                    onUpdate({
                      cert_issuer_kind:
                        option?.value === 'Issuer' ? 'Issuer' : 'ClusterIssuer',
                    })
                  }
                  options={issuerOptions}
                  styles={selectStyles<StringOption>()}
                  value={
                    issuerOptions.find(
                      option =>
                        option.value ===
                        (draft.cert_issuer_kind ?? 'ClusterIssuer'),
                    ) ?? null
                  }
                />
              </Field>
              <TextField
                error={errors.cert_issuer}
                hint={
                  <Hint content='Name of your cert-manager issuer (e.g., letsencrypt-prod)' />
                }
                label='Issuer Name *'
                name='cert_issuer'
                onChange={cert_issuer => onUpdate({ cert_issuer })}
                placeholder='letsencrypt-prod'
                value={draft.cert_issuer ?? ''}
              />
            </>
          )}
          <TextField
            hint={
              <Hint content='Leave empty to use default ingress class (e.g., nginx, traefik)' />
            }
            label='Ingress Class (Optional)'
            name='ingress_class'
            onChange={ingress_class => onUpdate({ ingress_class })}
            placeholder='nginx'
            value={draft.ingress_class ?? ''}
          />
          <TextField
            hint={
              <Hint content='JSON format: {"key": "value"}. Example: nginx.ingress.kubernetes.io/rewrite-target' />
            }
            label='Additional Annotations (Optional)'
            name='ingress_annotations'
            onChange={ingress_annotations => onUpdate({ ingress_annotations })}
            placeholder='{"key1": "value1", "key2": "value2"}'
            value={draft.ingress_annotations ?? ''}
          />
        </>
      )}
    </Stack>
  )
}
