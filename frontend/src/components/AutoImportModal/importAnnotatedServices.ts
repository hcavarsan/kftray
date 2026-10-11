import { invoke } from '@/lib/tauri'
import type { StoredConfig } from '@/types'

export interface AutoImportRequest {
  contextName: string
  kubeconfigPath: string
  namespace: string
  aliasAsDomain: boolean
  autoLoopback: boolean
}

// An empty namespace lets the backend list every namespace, and fall back to
// the context namespace when that list is forbidden.
export const importAnnotatedServices = async ({
  contextName,
  kubeconfigPath,
  namespace,
  aliasAsDomain,
  autoLoopback,
}: AutoImportRequest) => {
  const configs = await invoke<StoredConfig[]>(
    'get_services_with_annotations',
    {
      contextName,
      kubeconfigPath,
      namespace: namespace.trim() || null,
    },
  )
  const json = JSON.stringify(
    configs.map(config => ({
      ...config,
      domain_enabled: aliasAsDomain || config.domain_enabled,
      auto_loopback_address: autoLoopback || config.auto_loopback_address,
    })),
  )

  await invoke('import_configs_cmd', { json })
}
