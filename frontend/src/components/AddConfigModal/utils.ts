import { invoke } from '@tauri-apps/api/tauri'

import { Config, KubeContext } from '@/types'

export const fetchKubeContexts = (
  kubeConfig?: string,
): Promise<KubeContext[]> => {
  console.log('fetchKubeContexts', kubeConfig)

  return invoke('list_kube_contexts', { kubeconfig: kubeConfig })
}

export const trimConfigValues = (config: Config): Config => {
  const trimmedConfig = { ...config }
  // Define string keys of Config type
  const stringKeys = Object.keys(config).filter(
    key => typeof config[key as keyof Config] === 'string',
  ) as (keyof Config)[]

  // Trim string values
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
