import { invoke } from '@tauri-apps/api/core'

import { GitConfig } from '@/types'

export const gitService = {
  async saveCredentials(
    serviceName: string,
    accountName: string,
    credentials: GitConfig,
  ) {
    await invoke('store_key', {
      service: serviceName,
      name: accountName,
      password: JSON.stringify(credentials),
    })
  },

  async getCredentials(
    serviceName: string,
    accountName: string,
  ): Promise<GitConfig | null> {
    try {
      const credentialsString = await invoke<string>('get_key', {
        service: serviceName,
        name: accountName,
      })

      return JSON.parse(credentialsString)
    } catch (error) {
      if (
        error instanceof Error &&
        !error.toString().includes('No matching entry')
      ) {
        throw error
      }

      return null
    }
  },

  async importConfigs(credentials: GitConfig) {
    await invoke('import_configs_from_github', {
      repoUrl: credentials.repoUrl,
      configPath: credentials.configPath,
      useSystemCredentials: credentials.authMethod === 'system',
      flush: credentials.flush ?? false,
      githubToken:
        credentials.authMethod === 'token' ? credentials.token : null,
    })
  },

  async deleteCredentials(serviceName: string, accountName: string) {
    await invoke('delete_key', {
      service: serviceName,
      name: accountName,
    })
  },
}
