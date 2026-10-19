import { DEFAULT_KUBECONFIG } from '@/hooks/useKubeconfigPicker'
import type { Facet, StoredConfig } from '@/types'

import type { ConfigDraft, StringOption } from './types'

type ClearableField =
  | 'context'
  | 'namespace'
  | 'remote_port'
  | 'service'
  | 'target'

const PORT_TARGET_FIELDS: readonly ClearableField[] = [
  'service',
  'target',
  'remote_port',
]
const CASCADE: Partial<Record<keyof ConfigDraft, readonly ClearableField[]>> = {
  kubeconfig: ['context', 'namespace', ...PORT_TARGET_FIELDS],
  context: ['namespace', ...PORT_TARGET_FIELDS],
  namespace: PORT_TARGET_FIELDS,
  workload_type: PORT_TARGET_FIELDS,
  service: ['remote_port'],
  target: ['remote_port'],
}

export const applyDraftChange = (
  draft: ConfigDraft,
  change: Partial<ConfigDraft>,
): ConfigDraft => {
  const cleared: Partial<Record<ClearableField, string>> = {}
  for (const key of Object.keys(change) as (keyof ConfigDraft)[]) {
    if (change[key] !== draft[key]) {
      for (const field of CASCADE[key] ?? []) {
        cleared[field] = ''
      }
    }
  }
  return { ...draft, ...cleared, ...change }
}

export const trimConfigValues = (draft: ConfigDraft): ConfigDraft => ({
  ...draft,
  alias: draft.alias?.trim(),
  cert_issuer: draft.cert_issuer?.trim(),
  context: draft.context?.trim(),
  ingress_annotations: draft.ingress_annotations?.trim(),
  ingress_class: draft.ingress_class?.trim(),
  kubeconfig: draft.kubeconfig?.trim(),
  local_address: draft.local_address?.trim(),
  namespace: draft.namespace?.trim(),
  remote_address: draft.remote_address?.trim(),
  service: draft.service?.trim(),
  target: draft.target?.trim(),
})

export function toConfig(
  draft: ConfigDraft,
  initialConfig: StoredConfig | null,
): StoredConfig {
  const base = {
    alias: draft.alias ?? '',
    auto_loopback_address: draft.auto_loopback_address ?? false,
    context: draft.context ?? '',
    domain_enabled: draft.domain_enabled ?? false,
    http_logs_auto_cleanup: initialConfig?.http_logs_auto_cleanup,
    http_logs_enabled: initialConfig?.http_logs_enabled,
    http_logs_max_file_size: initialConfig?.http_logs_max_file_size,
    http_logs_retention_days: initialConfig?.http_logs_retention_days,
    id: initialConfig?.id ?? 0,
    kubeconfig: draft.kubeconfig ?? DEFAULT_KUBECONFIG,
    local_address: draft.local_address ?? '127.0.0.1',
    local_port: Number(draft.local_port),
    namespace: draft.namespace ?? '',
    protocol: draft.protocol ?? 'tcp',
    tags: draft.tags,
  }
  const remote_port = draft.remote_port ? Number(draft.remote_port) : undefined

  switch (draft.workload_type ?? 'service') {
    case 'pod':
      return {
        ...base,
        remote_port,
        service: '',
        target: draft.target ?? '',
        workload_type: 'pod',
      }
    case 'proxy':
      return {
        ...base,
        remote_address: draft.remote_address ?? '',
        remote_port,
        service: '',
        workload_type: 'proxy',
      }
    case 'expose': {
      const isPublic = draft.exposure_type === 'public'
      return {
        ...base,
        ...(isPublic && {
          cert_issuer: draft.cert_issuer,
          cert_issuer_kind: draft.cert_issuer_kind,
          cert_manager_enabled: draft.cert_manager_enabled,
          ingress_annotations: draft.ingress_annotations,
          ingress_class: draft.ingress_class,
        }),
        domain_enabled: isPublic && base.domain_enabled,
        exposure_protocol:
          draft.exposure_protocol === 'tcp' && !isPublic ? 'tcp' : undefined,
        exposure_type: draft.exposure_type,
        workload_type: 'expose',
      }
    }
    case 'service':
      return {
        ...base,
        remote_port,
        service: draft.service ?? '',
        workload_type: 'service',
      }
  }
}

const formatTag = (key: string, value: string) =>
  value ? `${key}=${value}` : key

export const tagsToOptions = (
  tags: Record<string, string> = {},
): StringOption[] =>
  Object.entries(tags).map(([key, value]) => {
    const tag = formatTag(key, value)
    return { label: tag, value: tag }
  })

const parseTag = (value: string): [string, string] => {
  const [key, ...rest] = value.split('=')
  return [key.trim().toLowerCase(), rest.join('=').trim()]
}

export const optionsToTags = (
  options: readonly StringOption[],
): Record<string, string> =>
  Object.fromEntries(options.map(({ value }) => parseTag(value)))

export const duplicateTagKey = (
  options: readonly StringOption[],
): string | null => {
  const seen = new Set<string>()
  for (const { value } of options) {
    const [key] = parseTag(value)
    if (seen.has(key)) {
      return key
    }
    seen.add(key)
  }
  return null
}

export const tagSuggestions = (facets: Facet[]): StringOption[] =>
  facets
    .filter(facet => facet.field.startsWith('tag:'))
    .flatMap(facet => {
      const key = facet.field.slice(4)
      return [key, ...facet.values.map(({ value }) => formatTag(key, value))]
    })
    .map(tag => ({ label: tag, value: tag }))

const TAG_KEY_PATTERN = /^[a-z0-9._/-]+$/

const tagsError = (tags: Record<string, string> = {}): string | null => {
  for (const [key, value] of Object.entries(tags)) {
    if (!TAG_KEY_PATTERN.test(key)) {
      return `Tag "${key}": use lowercase letters, numbers, . _ - /`
    }
    if (key.length > 63 || value.length > 128) {
      return `Tag "${key}" is too long (key 63, value 128 characters max)`
    }
    if (/[,=]/.test(value)) {
      return `Tag "${key}": values can't contain , or =`
    }
  }
  return null
}

const isPort = (value: string | undefined) => {
  const port = Number(value)
  return Number.isInteger(port) && port >= 1 && port <= 65535
}

export function validateDraft(draft: ConfigDraft): Record<string, string> {
  const errors: Record<string, string> = {}
  const required = (field: keyof ConfigDraft, label: string) => {
    if (!draft[field]?.toString().trim()) {
      errors[field] = `${label} is required`
    }
  }
  required('context', 'Context')
  required('namespace', 'Namespace')
  required('workload_type', 'Workload type')
  required('protocol', 'Protocol')

  const workloadType = draft.workload_type
  if (workloadType === 'expose') {
    required('alias', 'Domain')
    required('exposure_type', 'Exposure type')
    if (!isPort(draft.local_port)) {
      errors.local_port = 'Enter a port from 1 to 65535'
    }
    if (draft.exposure_type === 'public' && draft.cert_manager_enabled) {
      required('cert_issuer_kind', 'Issuer kind')
      required('cert_issuer', 'Issuer name')
    }
  } else {
    if (workloadType === 'proxy') {
      required('remote_address', 'Remote address')
    } else {
      required(
        workloadType === 'pod' ? 'target' : 'service',
        workloadType === 'pod' ? 'Pod label' : 'Service',
      )
    }
    if (!isPort(draft.remote_port)) {
      errors.remote_port = 'Enter a port from 1 to 65535'
    }
    if (draft.local_port && !isPort(draft.local_port)) {
      errors.local_port = 'Enter a port from 1 to 65535'
    }
  }

  const error = tagsError(draft.tags)
  if (error) {
    errors.tags = error
  }
  return errors
}
