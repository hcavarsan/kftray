import type { Config, StringOption } from '@/types'

export interface AddConfigModalProps {
  initialConfig: Config | null
  isEdit: boolean
  onClose: () => void
  onSave: (config: Config) => Promise<boolean>
}

export interface ConfigDraft
  extends Omit<Partial<Config>, 'local_port' | 'remote_port'> {
  local_port?: string
  remote_port?: string
}

export interface PortOption {
  label: string
  value: string
}

export type { StringOption }
