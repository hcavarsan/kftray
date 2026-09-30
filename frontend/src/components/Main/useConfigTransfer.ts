import { useCallback } from 'react'

import { useMutation, useQueryClient } from '@tanstack/react-query'
import { invoke } from '@tauri-apps/api/core'
import { open, save } from '@tauri-apps/plugin-dialog'
import { readTextFile, writeTextFile } from '@tauri-apps/plugin-fs'

import { toaster } from '@/components/ui/toaster'
import { configsQuery } from '@/hooks/useConfigs'
import { withNativeDialog } from '@/lib/nativeDialog'

const JSON_FILTERS = [{ name: 'JSON', extensions: ['json'] }]

export function useConfigTransfer() {
  const queryClient = useQueryClient()

  const exportMutation = useMutation({
    mutationFn: async () => {
      const json = await invoke('export_configs_cmd')

      if (typeof json !== 'string') {
        throw new Error('The exported config is not a string')
      }

      return withNativeDialog(async () => {
        const filePath = await save({
          defaultPath: 'configs.json',
          filters: JSON_FILTERS,
        })

        if (filePath) {
          await writeTextFile(filePath, json)
        }

        return Boolean(filePath)
      })
    },
    onSuccess: exported => {
      if (exported) {
        toaster.success({
          title: 'Success',
          description: 'Configuration exported successfully.',
          duration: 1000,
        })
      }
    },
    meta: {
      errorToast: { title: 'Failed to export configs', duration: 1000 },
    },
  })

  const importMutation = useMutation({
    mutationFn: async () => {
      const selected = await withNativeDialog(() =>
        open({ filters: JSON_FILTERS, multiple: false }),
      )

      if (!selected) {
        return false
      }
      const json = await readTextFile(selected)

      await invoke('import_configs_cmd', { json })
      await queryClient.invalidateQueries({ queryKey: configsQuery.queryKey })

      return true
    },
    onSuccess: imported => {
      if (imported) {
        toaster.success({
          title: 'Success',
          description: 'Configuration imported successfully.',
          duration: 1000,
        })
      }
    },
    meta: {
      errorToast: {
        title: 'Error',
        description: 'Failed to import configurations.',
        duration: 1000,
      },
    },
  })

  const { mutate: exportMutate } = exportMutation
  const { mutate: importMutate } = importMutation
  const exportConfigs = useCallback(() => exportMutate(), [exportMutate])
  const importConfigs = useCallback(() => importMutate(), [importMutate])

  return { exportConfigs, importConfigs }
}
