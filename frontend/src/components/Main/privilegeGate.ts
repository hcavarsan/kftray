import { invoke } from '@tauri-apps/api/core'

import type { PrivilegeNeed } from '@/components/PrivilegeDialog/types'
import type { StoredConfig } from '@/types'

export type PrivilegeDecision = 'install' | 'continue' | 'cancel'

export const PRIVILEGE_PROMPT_SETTING = 'privilege_prompt'

/** A start failure caused by a privileged local change being refused. */
export const PRIVILEGE_ERROR =
  /hostfile|hosts file|loopback address|permission denied/i

export interface PrivilegeGateDeps {
  prompt: (needs: PrivilegeNeed[]) => Promise<PrivilegeDecision>
  isSuppressed: () => Promise<boolean>
}

/**
 * Whether a start may go ahead. The backend reports what these configs
 * would need admin access for right now; with nothing needed, or the user
 * having asked not to be prompted, the start proceeds untouched. A preflight
 * that itself fails is not a reason to block a start that might work.
 */
export async function gateStart(
  configs: StoredConfig[],
  { prompt, isSuppressed }: PrivilegeGateDeps,
): Promise<boolean> {
  let needs: PrivilegeNeed[]

  try {
    needs = await invoke<PrivilegeNeed[]>('preflight_privileges', { configs })
  } catch {
    return true
  }
  if (needs.length === 0 || (await isSuppressed())) {
    return true
  }

  return (await prompt(needs)) !== 'cancel'
}
