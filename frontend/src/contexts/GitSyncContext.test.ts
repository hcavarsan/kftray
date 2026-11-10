import { createElement } from 'react'
import { renderToString } from 'react-dom/server'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'

import { invoke } from '@/lib/tauri'
import type { GitConfig } from '@/services/gitService'

import {
  type GitSyncContextValue,
  GitSyncProvider,
  useGitSync,
} from './GitSyncContext'

vi.mock('@/lib/tauri', () => ({ invoke: vi.fn() }))

const credentials: GitConfig = {
  repoUrl: 'https://github.com/example/configs',
  configPaths: ['configs.json'],
  authMethod: 'token',
  token: 'saved-token',
  pollingInterval: 5,
  flush: false,
}

let queryClient: QueryClient
let stored: GitConfig

interface ConsumerProps {
  onContext: (context: GitSyncContextValue) => void
}

function Consumer({ onContext }: ConsumerProps) {
  onContext(useGitSync())

  return null
}

function renderContext() {
  let context!: GitSyncContextValue

  renderToString(
    createElement(
      QueryClientProvider,
      { client: queryClient },
      createElement(
        GitSyncProvider,
        null,
        createElement(Consumer, {
          onContext: value => {
            context = value
          },
        }),
      ),
    ),
  )

  return context
}

async function settleMutations() {
  await vi.waitFor(() => {
    expect(queryClient.isMutating()).toBe(0)
  })
}

describe('Git sync settings', () => {
  beforeEach(() => {
    queryClient = new QueryClient({
      defaultOptions: {
        queries: { retry: false, gcTime: Infinity },
        mutations: { retry: false, gcTime: Infinity },
      },
    })
    stored = { ...credentials }
    queryClient.setQueryData(['git-sync-credentials'], stored)
    vi.mocked(invoke).mockReset()
    vi.mocked(invoke).mockImplementation(async (command, args) => {
      if (
        command === 'store_key' &&
        args &&
        'password' in args &&
        typeof args.password === 'string'
      ) {
        stored = JSON.parse(args.password)

        return undefined
      }
      if (command === 'import_configs_from_github') {
        throw new Error('Repository is unavailable')
      }
      throw new Error(`Unexpected command: ${command}`)
    })
  })

  afterEach(() => {
    queryClient.clear()
  })

  it('keeps polling off and retains credentials when import fails', async () => {
    renderContext().saveCredentials({ ...credentials, pollingInterval: 0 })
    await settleMutations()

    expect(stored).toEqual({ ...credentials, pollingInterval: 0 })
    expect(renderContext().credentials).toEqual(stored)
    expect(invoke).toHaveBeenCalledWith(
      'import_configs_from_github',
      expect.anything(),
    )
  })

  it('keeps the saved polling interval when credential storage fails', async () => {
    vi.mocked(invoke).mockRejectedValue(new Error('Credential store is locked'))

    renderContext().saveCredentials({ ...credentials, pollingInterval: 0 })
    await settleMutations()

    expect(stored).toEqual(credentials)
    expect(renderContext().credentials).toEqual(credentials)
    expect(invoke).not.toHaveBeenCalledWith(
      'import_configs_from_github',
      expect.anything(),
    )
  })
})
