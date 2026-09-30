import { useMutation, useQueryClient } from '@tanstack/react-query'
import { invoke } from '@tauri-apps/api/core'

import { toaster } from '@/components/ui/toaster'
import { errorMessage } from '@/lib/errors'

import type {
  CleanupMode,
  CleanupResult,
  CleanupSummary,
  ContextTarget,
  FlatResource,
} from './types'
import {
  CONTEXT_TIMEOUT_MS,
  RESOURCES_KEY,
  targetLabel,
  withTimeout,
} from './utils'

export function useResourceMutations(onCleaned: () => void) {
  const queryClient = useQueryClient()

  const invalidateResources = () =>
    queryClient.invalidateQueries({ queryKey: [RESOURCES_KEY] })

  const deleteMutation = useMutation({
    mutationFn: (resource: FlatResource) =>
      invoke('delete_kftray_resource', {
        contextName: resource.context,
        namespace: resource.namespace,
        resourceType: resource.resource_type,
        resourceName: resource.name,
        configId: resource.config_id,
        kubeconfig: resource.kubeconfig,
      }),
    onSuccess: (_, resource) => {
      toaster.success({
        title: 'Deleted',
        description: `Removed ${resource.name}`,
        duration: 2000,
      })
    },
    onError: error => {
      toaster.error({
        title: 'Error',
        description: `Failed to delete: ${errorMessage(error)}`,
        duration: 3000,
      })
    },
    onSettled: invalidateResources,
  })

  const cleanupMutation = useMutation({
    mutationFn: async ({
      mode,
      targets,
    }: {
      mode: CleanupMode
      targets: ContextTarget[]
    }): Promise<CleanupSummary> => {
      const command =
        mode === 'orphaned'
          ? 'cleanup_orphaned_kftray_resources'
          : 'cleanup_all_kftray_resources'
      const run = ({ context, kubeconfig }: ContextTarget) => {
        const request = invoke<CleanupResult>(command, {
          contextName: context,
          kubeconfig,
        })

        return targets.length === 1
          ? request
          : withTimeout(request, CONTEXT_TIMEOUT_MS * 2)
      }
      const results = await Promise.allSettled(targets.map(run))
      const summary: CleanupSummary = { removed: 0, errors: 0, failed: [] }

      for (const [index, result] of results.entries()) {
        if (result.status === 'fulfilled') {
          summary.removed += result.value.deleted
          summary.errors += result.value.errors
        } else {
          summary.failed.push(
            `${targetLabel(targets[index])}: ${errorMessage(result.reason)}`,
          )
        }
      }

      return summary
    },
    onSuccess: ({ removed, errors, failed }) => {
      const description = [`Removed ${removed} resources`]

      if (errors === 0 && failed.length === 0) {
        toaster.success({
          title: 'Done',
          description: description[0],
          duration: 2000,
        })
        onCleaned()

        return
      }
      if (errors > 0) {
        description.push(`${errors} ${errors === 1 ? 'error' : 'errors'}`)
      }
      if (failed.length > 0) {
        description.push(`Failed: ${failed.join('; ')}`)
      }
      const notify = removed > 0 ? toaster.warning : toaster.error

      notify({
        title: removed > 0 ? 'Partially done' : 'Cleanup failed',
        description: description.join('. '),
        duration: 5000,
      })
    },
    onError: error => {
      toaster.error({
        title: 'Error',
        description: `Cleanup failed: ${errorMessage(error)}`,
        duration: 3000,
      })
    },
    onSettled: invalidateResources,
  })

  return { deleteMutation, cleanupMutation, invalidateResources }
}
