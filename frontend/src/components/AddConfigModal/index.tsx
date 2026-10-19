import { type FormEvent, useMemo, useState } from 'react'

import { Stack } from '@chakra-ui/react'
import { useMutation, useQuery } from '@tanstack/react-query'

import { Button } from '@/components/ui/button'
import {
  AppDialog,
  AppDialogBody,
  AppDialogFooter,
  DialogCancelButton,
} from '@/components/ui/dialog'
import {
  useKubeContexts,
  useKubeNamespaces,
  useKubePodLabels,
  useKubePorts,
  useKubeServices,
} from '@/hooks/useKube'
import { DEFAULT_KUBECONFIG } from '@/hooks/useKubeconfigPicker'
import { invoke } from '@/lib/tauri'
import type { ConfigViewResult, StoredConfig } from '@/types'

import { CommonFields } from './CommonFields'
import { ExposeFields } from './ExposeFields'
import { KubeconfigControl } from './KubeconfigControl'
import { KubeTargetFields } from './KubeTargetFields'
import { ProxyFields } from './ProxyFields'
import { TagsField } from './TagsField'
import type { AddConfigModalProps, ConfigDraft, PortOption } from './types'
import {
  applyDraftChange,
  tagSuggestions,
  toConfig,
  trimConfigValues,
  validateDraft,
} from './utils'

const FORM_ID = 'add-config-form'

const emptyDraft: ConfigDraft = {
  alias: '',
  auto_loopback_address: false,
  context: '',
  domain_enabled: false,
  kubeconfig: DEFAULT_KUBECONFIG,
  local_address: '127.0.0.1',
  local_port: '',
  namespace: '',
  remote_address: '',
  remote_port: '',
  service: '',
  target: '',
}

const toDraft = (config: StoredConfig | null): ConfigDraft => {
  if (!config) {
    return { ...emptyDraft }
  }
  return {
    ...config,
    local_port: config.local_port ? String(config.local_port) : '',
    remote_port: config.remote_port ? String(config.remote_port) : '',
  }
}

export function AddConfigModal({
  initialConfig,
  isEdit,
  onClose,
  onSave,
}: AddConfigModalProps) {
  const [draft, setDraft] = useState<ConfigDraft>(() => toDraft(initialConfig))
  const [contextFocused, setContextFocused] = useState(false)
  const [duplicateTagError, setDuplicateTagError] = useState<string | null>(
    null,
  )
  const errors = validateDraft(draft)
  const shownErrors = Object.fromEntries(
    Object.entries(errors).filter(([field]) =>
      draft[field as keyof ConfigDraft]?.toString().trim(),
    ),
  )
  const kubeconfig = draft.kubeconfig ?? DEFAULT_KUBECONFIG
  const update = (next: Partial<ConfigDraft>) =>
    setDraft(current => applyDraftChange(current, next))
  const scope = {
    kubeconfig,
    context: draft.context,
    namespace: draft.namespace,
  }
  const contextQuery = useKubeContexts(
    kubeconfig,
    contextFocused || kubeconfig !== DEFAULT_KUBECONFIG,
  )
  const namespaceQuery = useKubeNamespaces(scope)
  const servicesQuery = useKubeServices(
    scope,
    draft.workload_type === 'service',
  )
  const podsQuery = useKubePodLabels(scope, draft.workload_type === 'pod')
  const target = draft.workload_type === 'pod' ? draft.target : draft.service
  const portQuery = useKubePorts(
    scope,
    draft.workload_type,
    target,
    draft.workload_type !== 'proxy' && draft.workload_type !== 'expose',
  )
  const saveMutation = useMutation({
    mutationFn: onSave,
    onSuccess: saved => {
      if (saved) {
        onClose()
      }
    },
  })
  const tagOptionsQuery = useQuery({
    queryKey: ['config-tag-options'],
    queryFn: () =>
      invoke<ConfigViewResult>('query_config_view_cmd', { view: null }),
    select: result => tagSuggestions(result.facets),
  })
  const portOptions = useMemo<PortOption[]>(
    () =>
      (portQuery.data ?? []).map(({ name, port }) => ({
        label: name ? `${name} (${port})` : String(port),
        value: String(port),
      })),
    [portQuery.data],
  )

  const handleSave = () => {
    const validationErrors = validateDraft(draft)
    if (
      Object.keys(validationErrors).length ||
      duplicateTagError ||
      saveMutation.isPending
    ) {
      return
    }
    saveMutation.mutate(toConfig(trimConfigValues(draft), initialConfig))
  }
  const handleSubmit = (event: FormEvent<HTMLFormElement>) => {
    event.preventDefault()
    if (document.activeElement?.getAttribute('aria-expanded') === 'true') {
      return
    }
    handleSave()
  }
  const workloadType = draft.workload_type
  const resourceOptions =
    workloadType === 'pod'
      ? (podsQuery.data ?? []).map(({ labels_str }) => ({
          label: labels_str,
          value: labels_str,
        }))
      : (servicesQuery.data ?? []).map(({ name }) => ({
          label: name,
          value: name,
        }))
  const resourceQuery = workloadType === 'pod' ? podsQuery : servicesQuery

  return (
    <AppDialog
      data-testid='add-config-dialog'
      title={isEdit ? 'Edit Configuration' : 'Add Configuration'}
      onClose={onClose}
      maxWidth='600px'
      height='96vh'
      headerExtra={
        <KubeconfigControl
          kubeconfig={kubeconfig}
          onChange={selected => update({ kubeconfig: selected })}
        />
      }
    >
      <AppDialogBody>
        <form id={FORM_ID} onSubmit={handleSubmit}>
          <Stack gap={2}>
            <CommonFields
              contextQuery={contextQuery}
              draft={draft}
              errors={shownErrors}
              namespaceQuery={namespaceQuery}
              onContextFocusChange={setContextFocused}
              onUpdate={update}
            />
            <TagsField
              error={duplicateTagError ?? shownErrors.tags}
              onChange={tags => update({ tags })}
              onDuplicate={setDuplicateTagError}
              options={tagOptionsQuery.data ?? []}
              tags={draft.tags}
            />
            {workloadType === 'expose' && (
              <ExposeFields
                draft={draft}
                errors={shownErrors}
                onUpdate={update}
              />
            )}
            {workloadType === 'proxy' && (
              <ProxyFields
                draft={draft}
                errors={shownErrors}
                onUpdate={update}
              />
            )}
            {workloadType !== 'expose' && workloadType !== 'proxy' && (
              <KubeTargetFields
                draft={draft}
                errors={
                  resourceQuery.error
                    ? {
                        ...shownErrors,
                        [workloadType === 'pod' ? 'target' : 'service']:
                          workloadType === 'pod'
                            ? 'Error fetching pods'
                            : 'Error fetching services',
                      }
                    : shownErrors
                }
                isLoading={resourceQuery.isLoading}
                noOptionsMessage={
                  resourceQuery.error
                    ? 'Type name manually'
                    : 'No results found'
                }
                onUpdate={update}
                options={resourceOptions}
                portError={portQuery.error ? 'Error fetching ports' : undefined}
                portOptions={portOptions}
                portsLoading={portQuery.isLoading}
                portsMessage={
                  portQuery.error
                    ? 'Type port number manually'
                    : 'No ports found'
                }
              />
            )}
          </Stack>
        </form>
      </AppDialogBody>
      <AppDialogFooter>
        <DialogCancelButton onClick={onClose} />
        <Button
          data-testid='config-save'
          bg='accent.solid'
          disabled={
            Boolean(duplicateTagError) || Object.keys(errors).length > 0
          }
          form={FORM_ID}
          height='28px'
          loading={saveMutation.isPending}
          loadingText='Saving...'
          size='xs'
          type='submit'
          _hover={{ bg: 'accent.solidHover' }}
        >
          {isEdit ? 'Save Changes' : 'Add Config'}
        </Button>
      </AppDialogFooter>
    </AppDialog>
  )
}
