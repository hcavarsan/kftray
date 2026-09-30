import { invoke } from '@tauri-apps/api/core'
import { open } from '@tauri-apps/plugin-dialog'

export async function withNativeDialog<T>(run: () => Promise<T>): Promise<T> {
  await invoke('open_save_dialog')
  try {
    return await run()
  } finally {
    await invoke('close_save_dialog')
  }
}

export const selectFile = () =>
  withNativeDialog(() => open({ multiple: false, directory: false }))
