import { invoke } from '@tauri-apps/api/core'

import type { Config, Facet, KubeContext, StringOption } from '@/types'

export const fetchKubeContexts = (
  kubeConfig?: string,
): Promise<KubeContext[]> => {
  console.log('fetchKubeContexts', kubeConfig)

  return invoke('list_kube_contexts', { kubeconfig: kubeConfig })
}

export const trimConfigValues = (config: Config): Config => {
  const trimmedConfig = { ...config }
  const stringKeys = Object.keys(config).filter(
    key => typeof config[key as keyof Config] === 'string',
  ) as (keyof Config)[]

  stringKeys.forEach(key => {
    const value = trimmedConfig[key]

    if (typeof value === 'string') {
      ;(trimmedConfig[key] as unknown) = value.trim()
    }
  })

  return trimmedConfig
}

export const validateFormFields = (
  fields: (string | number | undefined | null)[],
): boolean => {
  return fields.every(
    field => field !== null && field !== undefined && field !== '',
  )
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

export const optionsToTags = (
  options: readonly StringOption[],
): Record<string, string> =>
  Object.fromEntries(
    options.map(({ value }) => {
      const [key, ...rest] = value.split('=')

      return [key.trim().toLowerCase(), rest.join('=').trim()]
    }),
  )

export const tagSuggestions = (facets: Facet[]): StringOption[] =>
  facets
    .filter(facet => facet.field.startsWith('tag:'))
    .flatMap(facet => {
      const key = facet.field.slice(4)

      return [key, ...facet.values.map(({ value }) => formatTag(key, value))]
    })
    .map(tag => ({ label: tag, value: tag }))

const TAG_KEY_PATTERN = /^[a-z0-9._/-]+$/

export const tagsError = (tags: Record<string, string> = {}): string | null => {
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
