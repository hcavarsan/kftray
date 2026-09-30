import { type QueryMeta, useQuery } from '@tanstack/react-query'
import { invoke } from '@tauri-apps/api/core'

import type { KubeContext } from '@/types'

interface KubeScope {
  kubeconfig: string
  context?: string
  namespace?: string
}

export interface KubePort {
  name: string
  port: number
}

export const useKubeContexts = (
  kubeconfig: string,
  enabled = true,
  meta?: QueryMeta,
) =>
  useQuery({
    queryKey: ['kube-contexts', kubeconfig],
    queryFn: () => invoke<KubeContext[]>('list_kube_contexts', { kubeconfig }),
    enabled,
    meta,
  })

export const useKubeNamespaces = ({ kubeconfig, context }: KubeScope) =>
  useQuery({
    queryKey: ['kube-namespaces', kubeconfig, context],
    queryFn: () =>
      invoke<{ name: string }[]>('list_namespaces', {
        contextName: context,
        kubeconfig,
      }),
    enabled: Boolean(context),
  })

export const useKubeServices = (
  { kubeconfig, context, namespace }: KubeScope,
  enabled: boolean,
) =>
  useQuery({
    queryKey: ['kube-services', kubeconfig, context, namespace],
    queryFn: () =>
      invoke<{ name: string }[]>('list_services', {
        contextName: context,
        namespace,
        kubeconfig,
      }),
    enabled: enabled && Boolean(context && namespace),
  })

export const useKubePodLabels = (
  { kubeconfig, context, namespace }: KubeScope,
  enabled: boolean,
) =>
  useQuery({
    queryKey: ['kube-pods', kubeconfig, context, namespace],
    queryFn: () =>
      invoke<{ labels_str: string }[]>('list_pods', {
        contextName: context,
        namespace,
        kubeconfig,
      }),
    enabled: enabled && Boolean(context && namespace),
  })

export const useKubePorts = (
  { kubeconfig, context, namespace }: KubeScope,
  workloadType: string | undefined,
  target: string | undefined,
  enabled: boolean,
) =>
  useQuery({
    queryKey: [
      'kube-ports',
      kubeconfig,
      workloadType,
      context,
      namespace,
      target,
    ],
    queryFn: async (): Promise<KubePort[]> => {
      const ports = await invoke<
        { name: string | null; port: string | number | null }[]
      >('list_ports', {
        contextName: context,
        kubeconfig,
        namespace,
        serviceName: target,
      })
      return ports.flatMap(({ name, port }) => {
        const parsed = Number(port)
        return Number.isInteger(parsed) && parsed >= 1 && parsed <= 65535
          ? [{ name: name ?? '', port: parsed }]
          : []
      })
    },
    enabled: enabled && Boolean(context && namespace && target),
  })
