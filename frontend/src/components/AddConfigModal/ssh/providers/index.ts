import type { FC } from 'react'

import type { SshProvider, SshProviderType } from '@/types'

import { gcpBastion } from './gcpBastion'

export interface SshProviderFormProps {
  provider: SshProvider
  errors: Record<string, string>
  onChange: (provider: SshProvider) => void
}

export interface SshProviderUi {
  label: string
  badge: string
  empty: () => SshProvider
  normalize: (provider: SshProvider) => SshProvider
  Form: FC<SshProviderFormProps>
  validate: (provider: SshProvider) => Record<string, string>
  summary: (provider: SshProvider) => string
}

// Adding a provider: one entry here, plus its `type` in SshProviderType.
export const SSH_PROVIDERS: Record<SshProviderType, SshProviderUi> = {
  'gcp-bastion': gcpBastion,
}

// Configs from a newer kftray can carry a type this build does not know.
export const sshProviderUi = (type: string): SshProviderUi | undefined =>
  Object.hasOwn(SSH_PROVIDERS, type)
    ? SSH_PROVIDERS[type as SshProviderType]
    : undefined
