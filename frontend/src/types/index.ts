export type WorkloadType = 'service' | 'pod' | 'proxy' | 'expose'

export type Protocol = 'tcp' | 'udp'

export type ExposureType = 'cluster' | 'public'

export type CertIssuerKind = 'ClusterIssuer' | 'Issuer'

interface ConfigBase {
  id: number
  namespace?: string
  local_port?: number
  remote_port?: number
  context?: string
  protocol?: Protocol
  local_address?: string
  auto_loopback_address?: boolean
  alias?: string
  domain_enabled?: boolean
  kubeconfig?: string
  is_running: boolean
  http_logs_enabled?: boolean
  http_logs_max_file_size?: number
  http_logs_retention_days?: number
  http_logs_auto_cleanup?: boolean
  tags?: Record<string, string>
  service?: string
  target?: string
  remote_address?: string
}

interface ServiceConfig extends ConfigBase {
  workload_type: 'service'
  service: string
}

interface PodConfig extends ConfigBase {
  workload_type: 'pod'
  target: string
}

interface ProxyConfig extends ConfigBase {
  workload_type: 'proxy'
  remote_address: string
}

interface ExposeConfig extends ConfigBase {
  workload_type: 'expose'
  alias: string
  local_port: number
  exposure_type?: ExposureType
  cert_manager_enabled?: boolean
  cert_issuer?: string
  cert_issuer_kind?: CertIssuerKind
  ingress_class?: string
  ingress_annotations?: string
}

export type Config = ServiceConfig | PodConfig | ProxyConfig | ExposeConfig

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
