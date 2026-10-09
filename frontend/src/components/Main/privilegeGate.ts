import type { PrivilegeNeed } from '@/components/PrivilegeDialog/types'
import { invoke } from '@/lib/tauri'
import type { StoredConfig } from '@/types'

export type PrivilegeDecision = 'install' | 'continue' | 'cancel'

export const PRIVILEGE_PROMPT_SETTING = 'privilege_prompt'

/**
 * A start failure caused by a privileged local change being refused. The
 * alternatives are the exact messages `kube::start` and `network_utils`
 * produce for a hosts write the process may not make and for an elevation
 * prompt the user dismissed. The hosts branch carries the OS's own text
 * for `ErrorKind::PermissionDenied`: "Permission denied" on Unix, "Access
 * is denied" on Windows. A hosts write that failed for another reason (an
 * invalid alias, an I/O error) and a permission error from anywhere else
 * (a kubeconfig, a credential) are different failures and report as such:
 * elevation would not fix them.
 */
export const PRIVILEGE_ERROR =
  /Failed to (write to the hostfile for|add HTTPS hosts entries).*(Permission denied|Access is denied)|loopback address configuration cancelled|Address allocation cancelled by user|Network config failed: .*loopback address/

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
