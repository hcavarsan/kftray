import { useQueryClient } from '@tanstack/react-query'
import { invoke } from '@tauri-apps/api/core'
import { open, save } from '@tauri-apps/plugin-dialog'
import { readTextFile, writeTextFile } from '@tauri-apps/plugin-fs'

import { toaster } from '@/components/ui/toaster'
import { configsQuery } from '@/hooks/useConfigs'
import { errorMessage } from '@/lib/errors'
import { withNativeDialog } from '@/lib/nativeDialog'

const JSON_FILTERS = [{ name: 'JSON', extensions: ['json'] }]

export function useConfigTransfer() {
  const queryClient = useQueryClient()

  const exportConfigs = async () => {
    try {
      const json = await invoke<string>('export_configs_cmd')
      const exported = await withNativeDialog(async () => {
        const filePath = await save({
          defaultPath: 'configs.json',
          filters: JSON_FILTERS,
        })

        if (filePath) {
          await writeTextFile(filePath, json)
        }

        return Boolean(filePath)
      })

      if (!exported) {
        return
      }
      toaster.success({
        title: 'Success',
        description: 'Configuration exported successfully.',
        duration: 1000,
      })
    } catch (error) {
      toaster.error({
        title: 'Failed to export configs',
        description: errorMessage(error),
        duration: 1000,
      })
    }
  }

  const importConfigs = async () => {
    try {
      const selected = await withNativeDialog(() =>
        open({ filters: JSON_FILTERS, multiple: false }),
      )

      if (!selected) {
        return
      }
      const json = await readTextFile(selected)

      await invoke('import_configs_cmd', { json })
      await queryClient.invalidateQueries({ queryKey: configsQuery.queryKey })
      toaster.success({
        title: 'Success',
        description: 'Configuration imported successfully.',
        duration: 1000,
      })
    } catch (error) {
      toaster.error({
        title: 'Failed to import configs',
        description: errorMessage(error),
        duration: 1000,
      })
    }
  }

  return { exportConfigs, importConfigs }
}
