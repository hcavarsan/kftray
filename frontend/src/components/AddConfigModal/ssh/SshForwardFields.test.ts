import type * as ReactModule from 'react'
import { isValidElement, type ReactElement, type ReactNode } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import type * as ReactQueryModule from '@tanstack/react-query'
import type {
  MutateOptions,
  MutationObserver,
  MutationObserverOptions,
} from '@tanstack/react-query'

import { flushPromises } from '@/components/Main/testFixtures'
import { invoke } from '@/lib/tauri'
import type { SshTestStep, StoredConfig } from '@/types'

import type { ConfigDraft } from '../types'

import { SshForwardFields } from './SshForwardFields'

type SshObserver = MutationObserver<unknown, Error, StoredConfig>

const hooks = vi.hoisted(() => ({
  cursor: 0,
  observers: [] as SshObserver[],
}))

vi.mock('@/lib/tauri', () => ({ invoke: vi.fn() }))
vi.mock('react', async importOriginal => ({
  ...(await importOriginal<typeof ReactModule>()),
  useState: <T>(initial: T) => [initial, vi.fn()],
}))
vi.mock('@tanstack/react-query', async importOriginal => {
  const actual = await importOriginal<typeof ReactQueryModule>()
  const client = new actual.QueryClient()

  return {
    ...actual,
    useMutation: (
      options: MutationObserverOptions<unknown, Error, StoredConfig>,
    ) => {
      const slot = hooks.cursor

      hooks.cursor += 1
      if (!hooks.observers[slot]) {
        const created = new actual.MutationObserver<
          unknown,
          Error,
          StoredConfig
        >(client, options)

        created.subscribe(() => undefined)
        hooks.observers[slot] = created
      }

      const observer = hooks.observers[slot]

      observer.setOptions(options)

      return {
        ...observer.getCurrentResult(),
        mutate: (
          variables: StoredConfig,
          mutateOptions?: MutateOptions<unknown, Error, StoredConfig>,
        ) => {
          observer.mutate(variables, mutateOptions).catch(() => undefined)
        },
      }
    },
  }
})

interface TreeProps {
  children?: ReactNode
  'data-testid'?: string
  onChange?: (value: string) => void
  onClick?: () => void
}

const sshDraft: ConfigDraft = {
  local_port: '5432',
  remote_address: 'db.internal',
  remote_port: '5432',
  ssh_provider: {
    instance: 'jumper',
    project: 'my-project',
    type: 'gcp-bastion',
    zone: 'us-central1-b',
  },
  workload_type: 'ssh-port-forward',
}

const hostKeyMismatch: SshTestStep[] = [
  { step: 'route', status: 'ok' },
  { step: 'transport', status: 'ok' },
  {
    step: 'handshake',
    status: 'failed',
    message: 'host key changed',
    host_key: { known: 'SHA256:old', presented: 'SHA256:new' },
  },
  { step: 'auth', status: 'skipped' },
  { step: 'channel', status: 'skipped' },
]

const reachable: SshTestStep[] = [
  { step: 'route', status: 'ok' },
  { step: 'transport', status: 'ok' },
  { step: 'handshake', status: 'ok' },
  { step: 'auth', status: 'ok' },
  { step: 'channel', status: 'ok' },
]

const findByTestId = (
  node: ReactNode,
  testId: string,
): ReactElement<TreeProps> | undefined => {
  if (Array.isArray(node)) {
    return node.map(child => findByTestId(child, testId)).find(Boolean)
  }
  if (!isValidElement<TreeProps>(node)) {
    return undefined
  }

  return node.props['data-testid'] === testId
    ? node
    : findByTestId(node.props.children, testId)
}

const mountFields = () => {
  let draft = sshDraft
  const onUpdate = (change: Partial<ConfigDraft>) => {
    draft = { ...draft, ...change }
  }
  const find = (testId: string) => {
    hooks.cursor = 0

    return findByTestId(
      SshForwardFields({ draft, errors: {}, initialConfig: null, onUpdate }),
      testId,
    )
  }

  return {
    find,
    press: (testId: string) => {
      const onClick = find(testId)?.props.onClick

      if (!onClick) {
        throw new Error(`${testId} is not a rendered button`)
      }
      onClick()
    },
    edit: (testId: string, value: string) => {
      const onChange = find(testId)?.props.onChange

      if (!onChange) {
        throw new Error(`${testId} is not a rendered field`)
      }
      onChange(value)
    },
  }
}

const testCalls = () =>
  vi
    .mocked(invoke)
    .mock.calls.filter(([command]) => command === 'test_ssh_forward_cmd')

describe('SshForwardFields host key reset', () => {
  let forget: PromiseWithResolvers<undefined>

  beforeEach(() => {
    vi.useFakeTimers()
    hooks.observers.length = 0
    forget = Promise.withResolvers()
    vi.mocked(invoke).mockReset()
    vi.mocked(invoke).mockImplementation(async command => {
      if (command === 'forget_ssh_host_key_cmd') {
        return forget.promise
      }

      return testCalls().length === 1 ? hostKeyMismatch : reachable
    })
  })

  afterEach(() => {
    vi.useRealTimers()
  })

  it('retests the same config once the host key is forgotten', async () => {
    const fields = mountFields()

    fields.press('config-ssh-test')
    await flushPromises()
    fields.press('config-ssh-forget-host-key')
    forget.resolve(undefined)
    await flushPromises()

    expect(testCalls()).toHaveLength(2)
    expect(testCalls()[1]).toEqual(testCalls()[0])
    expect(fields.find('config-ssh-test-result')).toBeDefined()
    expect(fields.find('config-ssh-forget-host-key')).toBeUndefined()
  })

  it('does not retest or show a result when the destination changed during the forget', async () => {
    const fields = mountFields()

    fields.press('config-ssh-test')
    await flushPromises()
    fields.press('config-ssh-forget-host-key')
    fields.edit('config-ssh-remote-address', 'other.internal')
    forget.resolve(undefined)
    await flushPromises()

    expect(invoke).toHaveBeenCalledWith('forget_ssh_host_key_cmd', {
      config: expect.objectContaining({ remote_address: 'db.internal' }),
    })
    expect(testCalls()).toHaveLength(1)
    expect(fields.find('config-ssh-test-result')).toBeUndefined()
  })
})
