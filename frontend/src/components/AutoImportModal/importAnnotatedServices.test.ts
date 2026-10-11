import { beforeEach, describe, expect, it, vi } from 'vitest'

import { invoke } from '@/lib/tauri'

import {
  type AutoImportRequest,
  importAnnotatedServices,
} from './importAnnotatedServices'

vi.mock('@/lib/tauri', () => ({ invoke: vi.fn() }))

const request = (namespace: string): AutoImportRequest => ({
  contextName: 'dev',
  kubeconfigPath: 'default',
  namespace,
  aliasAsDomain: false,
  autoLoopback: false,
})

describe('importAnnotatedServices', () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset()
    vi.mocked(invoke).mockResolvedValue([])
  })

  it('sends a blank namespace as none so every namespace is checked', async () => {
    await importAnnotatedServices(request('   '))

    expect(invoke).toHaveBeenCalledWith('get_services_with_annotations', {
      contextName: 'dev',
      kubeconfigPath: 'default',
      namespace: null,
    })
  })

  it('sends the namespace without surrounding spaces', async () => {
    await importAnnotatedServices(request(' team-a '))

    expect(invoke).toHaveBeenCalledWith('get_services_with_annotations', {
      contextName: 'dev',
      kubeconfigPath: 'default',
      namespace: 'team-a',
    })
  })

  it('imports nothing when the namespace cannot be read', async () => {
    vi.mocked(invoke).mockRejectedValueOnce(
      'cannot list services in namespace "team-a"',
    )

    await expect(importAnnotatedServices(request('team-a'))).rejects.toBe(
      'cannot list services in namespace "team-a"',
    )
    expect(invoke).not.toHaveBeenCalledWith(
      'import_configs_cmd',
      expect.anything(),
    )
  })
})
