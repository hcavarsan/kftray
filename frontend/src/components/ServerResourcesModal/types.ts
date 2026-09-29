interface ServerResource {
  resource_type: string
  name: string
  namespace: string
  config_id: string | null
  is_orphaned: boolean
  age: string
  status: string
}

export interface NamespaceGroup {
  namespace: string
  resources: ServerResource[]
}

export interface ContextTarget {
  context: string
  kubeconfig: string | null
}

export interface ContextOption {
  label: string
  value: string
  targets: ContextTarget[]
}

export interface FlatResource extends ServerResource, ContextTarget {
  key: string
}

export type CleanupMode = 'orphaned' | 'all'
