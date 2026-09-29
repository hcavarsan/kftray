type WorkloadType = 'service' | 'pod' | 'proxy' | 'expose'

export type Protocol = 'tcp' | 'udp'

type ExposureType = 'cluster' | 'public'

type CertIssuerKind = 'ClusterIssuer' | 'Issuer'

export interface Config {
  id: number
  service: string
  namespace: string
  local_port: number
  local_address: string
  auto_loopback_address: boolean
  domain_enabled: boolean
  remote_port?: number
  context: string
  alias: string
  remote_address: string
  workload_type: WorkloadType
  target: string
  protocol: Protocol
  kubeconfig: string
  is_running: boolean
  http_logs_enabled?: boolean
  exposure_type?: ExposureType
  cert_manager_enabled?: boolean
  cert_issuer?: string
  cert_issuer_kind?: CertIssuerKind
  ingress_class?: string
  ingress_annotations?: string
  tags?: Record<string, string>
}

export interface ViewCondition {
  field: string
  values: string[]
}

export interface ConfigView {
  group_by: string | null
  filters: ViewCondition[]
}

interface ConfigGroup {
  key: string | null
  label: string
  config_ids: number[]
}

interface FacetValue {
  value: string
  count: number
}

export interface Facet {
  field: string
  count: number
  values: FacetValue[]
}

export interface ConfigViewResult {
  view: ConfigView
  groups: ConfigGroup[]
  facets: Facet[]
}

export interface ResolvedGroup {
  id: string
  label: string
  configs: Config[]
}

export type PortForwardAction = 'starting' | 'stopping' | 'saving' | 'deleting'

export type PortForwardToggleAction = 'starting' | 'stopping'

export interface PendingConfigAction {
  action: PortForwardAction
  token: number
  timedOut?: boolean
}

export interface PortForwardResponse {
  id: number | null
  service: string
  namespace: string
  local_port: number
  remote_port: number
  context: string
  stdout: string
  stderr: string
  status: number
  protocol: string
}

export type AuthMethod = 'none' | 'system' | 'token'

export interface KubeContext {
  name: string
  cluster?: string
  user?: string
}

export interface StringOption {
  label: string
  value: string
}
