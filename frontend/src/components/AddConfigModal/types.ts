import type {
  CertIssuerKind,
  Config,
  ExposureType,
  Protocol,
  StringOption,
  WorkloadType,
} from '@/types'

export interface AddConfigModalProps {
  initialConfig: Config | null
  isEdit: boolean
  onClose: () => void
  onSave: (config: Config) => Promise<boolean>
}

export interface ConfigDraft {
  alias?: string
  auto_loopback_address?: boolean
  cert_issuer?: string
  cert_issuer_kind?: CertIssuerKind
  cert_manager_enabled?: boolean
  context?: string
  domain_enabled?: boolean
  exposure_type?: ExposureType
  ingress_annotations?: string
  ingress_class?: string
  kubeconfig?: string
  local_address?: string
  local_port?: string
  namespace?: string
  protocol?: Protocol
  remote_address?: string
  remote_port?: string
  service?: string
  tags?: Record<string, string>
  target?: string
  workload_type?: WorkloadType
}

export interface PortOption {
  label: string
  value: string
}

export type { StringOption }
