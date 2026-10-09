import type { InvokeArgs, InvokeOptions } from '@tauri-apps/api/core'
import { invoke as tauriInvoke } from '@tauri-apps/api/core'

import { attachProblemReportContext } from './problemReports'
import { captureInvokeFailure, invokeSpan } from './telemetry'

// Every call to the Rust side goes through here so it can be timed and its
// failure reported. Command names are fixed strings in the code; arguments
// and results never reach telemetry.
export const invoke = <T>(
  command: string,
  args?: InvokeArgs,
  options?: InvokeOptions,
): Promise<T> =>
  invokeSpan(command, () =>
    tauriInvoke<T>(command, args, options).catch((error: unknown) => {
      throw attachProblemReportContext(error, captureInvokeFailure(command))
    }),
  )
