import { useCallback, useRef, useState } from 'react'

import { useQueryClient } from '@tanstack/react-query'
import { invoke } from '@tauri-apps/api/core'

import type { PrivilegeNeed } from '@/components/PrivilegeDialog/types'
import type { StoredConfig } from '@/types'

import {
  gateStart,
  PRIVILEGE_PROMPT_SETTING,
  type PrivilegeDecision,
} from './privilegeGate'

const SUPPRESSED_KEY = ['setting', PRIVILEGE_PROMPT_SETTING]

export interface PrivilegePrompt {
  needs: PrivilegeNeed[]
  /** Set when a start already failed for lack of privileges. */
  error: string | null
  resolve: (decision: PrivilegeDecision) => void
}

export function usePrivilegeGate() {
  const queryClient = useQueryClient()
  const [prompt, setPrompt] = useState<PrivilegePrompt | null>(null)
  // Prompts are shown one at a time: a second start reaching the gate while
  // a dialog is open waits for that dialog to close, then gets its own.
  // Replacing the visible prompt would drop the first start's resolver and
  // leave it waiting forever. `activeRef` mirrors `prompt` so the decision
  // is made outside a state updater, which must stay pure.
  const activeRef = useRef<PrivilegePrompt | null>(null)
  const queueRef = useRef<PrivilegePrompt[]>([])

  const show = useCallback((next: PrivilegePrompt | null) => {
    activeRef.current = next
    setPrompt(next)
  }, [])

  const ask = useCallback(
    (needs: PrivilegeNeed[], error: string | null) =>
      new Promise<PrivilegeDecision>(resolve => {
        const next = { needs, error, resolve }

        if (activeRef.current) {
          queueRef.current.push(next)
        } else {
          show(next)
        }
      }),
    [show],
  )

  const isSuppressed = useCallback(
    () =>
      queryClient
        .fetchQuery({
          queryKey: SUPPRESSED_KEY,
          queryFn: async () =>
            (await invoke<string | null>('get_setting_value', {
              key: PRIVILEGE_PROMPT_SETTING,
            })) === 'never',
          staleTime: Number.POSITIVE_INFINITY,
        })
        .catch(() => false),
    [queryClient],
  )

  const ensurePrivileges = useCallback(
    (configs: StoredConfig[]) =>
      gateStart(configs, { isSuppressed, prompt: needs => ask(needs, null) }),
    [ask, isSuppressed],
  )

  /**
   * After a start was refused: the same dialog, now with the error and a
   * retry. The resource list is re-read so it reflects what still needs
   * privileges, which may be nothing if the helper came up meanwhile.
   */
  const askAfterRefusal = useCallback(
    async (configs: StoredConfig[], error: string) => {
      const needs = await invoke<PrivilegeNeed[]>('preflight_privileges', {
        configs,
      }).catch(() => [])

      return ask(needs, error)
    },
    [ask],
  )

  const resolvePrompt = useCallback(
    (decision: PrivilegeDecision) => {
      activeRef.current?.resolve(decision)
      show(queueRef.current.shift() ?? null)
    },
    [show],
  )

  const suppressPrompt = useCallback(async () => {
    await invoke('set_setting_value', {
      key: PRIVILEGE_PROMPT_SETTING,
      value: 'never',
    })
    queryClient.setQueryData(SUPPRESSED_KEY, true)
  }, [queryClient])

  return {
    prompt,
    ensurePrivileges,
    askAfterRefusal,
    resolvePrompt,
    suppressPrompt,
  }
}
