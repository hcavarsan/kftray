import { showErrorToast } from '@/components/ui/error-toast'
import { toaster } from '@/components/ui/toaster'
import { errorMessage } from '@/lib/errors'
import type { Config, PortForwardToggleAction } from '@/types'

import { PRIVILEGE_ERROR, type PrivilegeDecision } from './privilegeGate'
import type { RunForwardCommand } from './useForwardCommand'
import type { PendingActions } from './usePendingActions'

export interface ToggleDeps {
  pending: Pick<PendingActions, 'isBusy' | 'markPending' | 'clearPending'>
  ensurePrivileges: (configs: Config[]) => Promise<boolean>
  askAfterRefusal: (
    configs: Config[],
    message: string,
  ) => Promise<PrivilegeDecision>
  runForwardCommand: RunForwardCommand
  refreshConfigs: () => Promise<void>
}

export async function executeToggle(
  {
    pending,
    ensurePrivileges,
    askAfterRefusal,
    runForwardCommand,
    refreshConfigs,
  }: ToggleDeps,
  config: Config,
  action: PortForwardToggleAction,
) {
  const { isBusy, markPending, clearPending } = pending

  if (isBusy(config.id)) {
    toaster.error({
      title: 'Error',
      description: 'This configuration is busy. Try again once it settles.',
      duration: 1000,
    })

    return
  }
  let token = markPending(config.id, action)

  if (action === 'starting') {
    let allowed = false

    try {
      allowed = await ensurePrivileges([config])
    } finally {
      if (!allowed) {
        clearPending(config.id, token)
      }
    }
    if (!allowed) {
      return
    }
  }
  // A start refused for lack of privileges is offered once more after
  // the user has had the chance to install the helper or accept the
  // prompts. The row is released between the two attempts so the retry
  // can reserve it again. A second refusal is reported as a plain
  // failure.
  for (let attempt = 0; attempt < 2; attempt += 1) {
    if (attempt > 0) {
      token = markPending(config.id, action)
    }
    let refused: string | null = null

    try {
      await runForwardCommand(config, action, token)
    } catch (error) {
      await refreshConfigs()
      const message = errorMessage(error)

      if (
        action === 'starting' &&
        attempt === 0 &&
        PRIVILEGE_ERROR.test(message)
      ) {
        refused = message
      } else {
        showErrorToast(error, {
          errorToast: {
            title:
              action === 'starting'
                ? 'Error starting port forwarding'
                : 'Error stopping port forwarding',
            duration: 1000,
          },
        })
      }
    } finally {
      clearPending(config.id, token)
      void refreshConfigs()
    }
    if (
      refused === null ||
      (await askAfterRefusal([config], refused)) === 'cancel'
    ) {
      return
    }
  }
}
