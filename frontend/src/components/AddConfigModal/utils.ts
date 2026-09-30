import type { Facet } from '@/types'

import type { ConfigDraft, StringOption } from './types'

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
