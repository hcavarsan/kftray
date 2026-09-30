import { invoke } from '@tauri-apps/api/core'

import type { AuthMethod } from '@/types'

export interface GitConfig {
  repoUrl: string
  configPaths: string[]
  authMethod: AuthMethod
  token?: string
  pollingInterval: number
  flush?: boolean
}

type StoredGitConfig = Omit<GitConfig, 'configPaths'> & {
  configPaths?: string[]
  configPath?: string
}

const SERVICE_NAME = 'kftray'
const ACCOUNT_NAME = 'github_config'

export const gitService = {
  async getCredentials(): Promise<GitConfig | null> {
    const credentialsString = await invoke<string | null>('get_key', {
      service: SERVICE_NAME,
      name: ACCOUNT_NAME,
    })

    if (!credentialsString) {
      return null
    }
    const { configPath, configPaths, ...stored }: StoredGitConfig =
      JSON.parse(credentialsString)

    return {
      ...stored,
      configPaths: configPaths ?? (configPath ? [configPath] : []),
    }
  },

  async saveCredentials(credentials: GitConfig) {
    await invoke('store_key', {
      service: SERVICE_NAME,
      name: ACCOUNT_NAME,
      password: JSON.stringify(credentials),
    })
  },

  async deleteCredentials() {
    await invoke('delete_key', {
      service: SERVICE_NAME,
      name: ACCOUNT_NAME,
    })
  },

  async importConfigs(credentials: GitConfig) {
    await invoke('import_configs_from_github', {
      repoUrl: credentials.repoUrl,
      configPaths: credentials.configPaths,
      useSystemCredentials: credentials.authMethod === 'system',
      flush: credentials.flush ?? false,
      githubToken:
        credentials.authMethod === 'token' ? credentials.token : null,
    })
  },
}
