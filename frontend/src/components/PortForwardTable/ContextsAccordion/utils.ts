import { invoke } from '@tauri-apps/api/core'

import { KubeContext } from '@/types'

export const fetchKubeContexts = (
  kubeConfig?: string,
): Promise<KubeContext[]> => {
  console.log('fetchKubeContexts', kubeConfig)

  return invoke('list_kube_contexts', { kubeconfig: kubeConfig })
}
