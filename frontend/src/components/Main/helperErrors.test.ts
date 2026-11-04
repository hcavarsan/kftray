import type * as React from 'react'
import type { ComponentProps, ReactNode } from 'react'
import { Children, isValidElement } from 'react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import type * as ReactQuery from '@tanstack/react-query'
import type {
  MutationObserver,
  MutationObserverOptions,
} from '@tanstack/react-query'
import { QueryClient } from '@tanstack/react-query'

import { HelperModal } from '@/components/HelperModal'
import { PrivilegeDialog } from '@/components/PrivilegeDialog'
import { invoke } from '@/lib/tauri'

import { Main } from './index'
import {
  createPrivilegePromptQueue,
  type PrivilegePrompt,
  type PrivilegePromptQueue,
} from './privilegePromptQueue'

const hooks = vi.hoisted(() => ({
  states: [] as unknown[],
  cursor: 0,
  client: undefined as unknown,
  prompt: null as PrivilegePrompt | null,
  queue: undefined as unknown as PrivilegePromptQueue,
  subscriptions: [] as (() => void)[],
}))

vi.mock('react', async importOriginal => ({
  ...(await importOriginal<typeof React>()),
  useCallback: <T>(callback: T) => callback,
  useMemo: <T>(calculate: () => T) => calculate(),
  useState: <T>(initial: T) => {
    const index = hooks.cursor++

    if (!(index in hooks.states)) {
      hooks.states[index] = initial
    }

    return [
      hooks.states[index],
      (next: T | ((previous: T) => T)) => {
        hooks.states[index] =
          typeof next === 'function'
            ? (next as (previous: T) => T)(hooks.states[index] as T)
            : next
      },
    ]
  },
}))
vi.mock('@tanstack/react-query', async importOriginal => {
  const original = await importOriginal<typeof ReactQuery>()

  return {
    ...original,
    useQueryClient: () => hooks.client,
    useQuery: () => ({
      isFetched: true,
      data: { running: false },
    }),
    useMutation: (options: MutationObserverOptions<unknown, Error, void>) => {
      const index = hooks.cursor++
      let observer = hooks.states[index] as
        | MutationObserver<unknown, Error, void>
        | undefined

      if (!observer) {
        observer = new original.MutationObserver(
          hooks.client as QueryClient,
          options,
        )
        hooks.states[index] = observer
        hooks.subscriptions.push(observer.subscribe(() => undefined))
      }
      observer.setOptions(options)

      return {
        ...observer.getCurrentResult(),
        mutate: (...args: Parameters<typeof observer.mutate>) => {
          void observer.mutate(...args).catch(() => undefined)
        },
      }
    },
  }
})
vi.mock('@/lib/tauri', () => ({ invoke: vi.fn() }))
vi.mock('@/components/ui/toaster', () => ({
  toaster: { success: vi.fn() },
}))
vi.mock('@/components/Footer', () => ({ Footer: () => null }))
vi.mock('@/components/PortForwardTable', () => ({
  PortForwardTable: () => null,
}))
vi.mock('./useConfigTransfer', () => ({ useConfigTransfer: () => ({}) }))
vi.mock('./usePortForwarding', () => ({
  usePortForwarding: () => ({
    privileges: {
      prompt: hooks.prompt,
      resolvePrompt: (
        decision: Parameters<PrivilegePromptQueue['resolve']>[0],
      ) => hooks.queue.resolve(decision),
    },
  }),
}))

function elements(node: ReactNode): Record<string, unknown>[] {
  return Children.toArray(node).flatMap(child => {
    if (!isValidElement<{ children?: ReactNode }>(child)) {
      return []
    }

    return [child.props, ...elements(child.props.children)]
  })
}

function text(node: ReactNode): string {
  return Children.toArray(node)
    .map(child => {
      if (isValidElement<{ children?: ReactNode }>(child)) {
        return text(child.props.children)
      }

      return String(child)
    })
    .join(' ')
}

function privilegeDialog() {
  hooks.cursor = 0
  const props = elements(Main()).find(element => 'installError' in element)

  if (!props) {
    throw new Error('Privilege dialog is not open')
  }

  const dialog = props as unknown as ComponentProps<typeof PrivilegeDialog>

  return { ...dialog, text: text(PrivilegeDialog(dialog)) }
}

function helperModal() {
  hooks.cursor = 0

  return HelperModal({ onClose: vi.fn() })
}

function helperAction(action: 'Install' | 'Uninstall') {
  const button = elements(helperModal()).find(
    element => element.children === action,
  ) as { onClick: () => void }

  button.onClick()

  if (action === 'Uninstall') {
    const confirmation = elements(helperModal()).find(
      element => element.title === 'Uninstall kftray-helper?',
    ) as { onConfirm: () => void }

    confirmation.onConfirm()
  }
}

beforeEach(() => {
  hooks.states = []
  hooks.client = new QueryClient({
    defaultOptions: { mutations: { retry: false, gcTime: 0 } },
  })
  hooks.prompt = null
  hooks.queue = createPrivilegePromptQueue(prompt => {
    hooks.prompt = prompt
  })
  vi.mocked(invoke).mockReset()
})

afterEach(() => {
  for (const unsubscribe of hooks.subscriptions.splice(0)) {
    unsubscribe()
  }
  const client = hooks.client as QueryClient

  client.clear()
})

describe('helper action errors', () => {
  it.each(['cancel', 'continue'] as const)(
    'clears a failed install before the prompt after %s',
    async decision => {
      vi.mocked(invoke).mockRejectedValue(new Error('Install was denied'))
      const first = hooks.queue.ask([], null)

      privilegeDialog().onInstall()
      await vi.waitFor(() => {
        expect(privilegeDialog().text).toContain('Install was denied')
      })

      const second = hooks.queue.ask([], null)
      const dialog = privilegeDialog()

      if (decision === 'cancel') {
        dialog.onCancel()
      } else {
        dialog.onContinue()
      }

      expect(await first).toBe(decision)
      expect(privilegeDialog().text).not.toContain('Install was denied')
      privilegeDialog().onCancel()
      expect(await second).toBe('cancel')
    },
  )

  it.each([
    ['Install', 'Uninstall', 'install_helper'],
    ['Uninstall', 'Install', 'remove_helper'],
  ] as const)(
    'clears a failed %s after a successful %s',
    async (failedAction, nextAction, command) => {
      vi.mocked(invoke).mockImplementation(async name => {
        if (name === command) {
          throw new Error('Admin prompt was denied')
        }

        return true
      })
      helperAction(failedAction)
      await vi.waitFor(() => {
        expect(text(helperModal())).toContain('Admin prompt was denied')
      })
      helperAction(nextAction)
      await vi.waitFor(() => {
        expect(text(helperModal())).not.toContain('Admin prompt was denied')
        expect(
          (hooks.client as QueryClient).getMutationCache().getAll().at(-1)
            ?.state.status,
        ).toBe('success')
      })
    },
  )

  it('shows the latest action failure instead of the previous failure', async () => {
    vi.mocked(invoke).mockImplementation(async command => {
      throw new Error(
        command === 'install_helper' ? 'Install was denied' : 'Removal failed',
      )
    })
    helperAction('Install')
    await vi.waitFor(() => {
      expect(text(helperModal())).toContain('Install was denied')
    })
    helperAction('Uninstall')
    await vi.waitFor(() => {
      const message = text(helperModal())

      expect(message).toContain('Removal failed')
      expect(message).not.toContain('Install was denied')
    })
  })
})
