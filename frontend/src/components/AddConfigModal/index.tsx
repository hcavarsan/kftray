import { useMemo, useState } from 'react'

import { Stack } from '@chakra-ui/react'
import { useMutation, useQuery } from '@tanstack/react-query'
import { invoke } from '@tauri-apps/api/core'

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
import type { ConfigViewResult, StoredConfig } from '@/types'

import { CommonFields } from './CommonFields'
import { ExposeFields } from './ExposeFields'
import { KubeconfigControl } from './KubeconfigControl'
import { KubeTargetFields } from './KubeTargetFields'
import { ProxyFields } from './ProxyFields'
import { TagsField } from './TagsField'
import type { AddConfigModalProps, ConfigDraft, PortOption } from './types'
import { tagSuggestions, trimConfigValues, validateDraft } from './utils'

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

function toConfig(
  draft: ConfigDraft,
  initialConfig: StoredConfig | null,
): StoredConfig {
  const base = {
    alias: draft.alias ?? '',
    auto_loopback_address: draft.auto_loopback_address ?? false,
    context: draft.context ?? '',
    domain_enabled: draft.domain_enabled ?? false,
    http_logs_auto_cleanup: initialConfig?.http_logs_auto_cleanup,
    http_logs_enabled: initialConfig?.http_logs_enabled,
    http_logs_max_file_size: initialConfig?.http_logs_max_file_size,
    http_logs_retention_days: initialConfig?.http_logs_retention_days,
    id: initialConfig?.id ?? 0,
    kubeconfig: draft.kubeconfig ?? DEFAULT_KUBECONFIG,
    local_address: draft.local_address ?? '127.0.0.1',
    local_port: Number(draft.local_port),
    namespace: draft.namespace ?? '',
    protocol: draft.protocol ?? 'tcp',
    tags: draft.tags,
  }
  const remote_port = draft.remote_port ? Number(draft.remote_port) : undefined

  switch (draft.workload_type ?? 'service') {
    case 'pod':
      return {
        ...base,
        remote_port,
        service: '',
        target: draft.target ?? '',
        workload_type: 'pod',
      }
    case 'proxy':
      return {
        ...base,
        remote_address: draft.remote_address ?? '',
        remote_port,
        service: '',
        workload_type: 'proxy',
      }
    case 'expose':
      return {
        ...base,
        ...(draft.exposure_type === 'public' && {
          cert_issuer: draft.cert_issuer,
          cert_issuer_kind: draft.cert_issuer_kind,
          cert_manager_enabled: draft.cert_manager_enabled,
          ingress_annotations: draft.ingress_annotations,
          ingress_class: draft.ingress_class,
        }),
        exposure_type: draft.exposure_type,
        workload_type: 'expose',
      }
    case 'service':
      return {
        ...base,
        remote_port,
        service: draft.service ?? '',
        workload_type: 'service',
      }
  }
}

function AddConfigModal({
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
    setDraft(current => ({ ...current, ...next }))
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
            <ProxyFields draft={draft} errors={shownErrors} onUpdate={update} />
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
                resourceQuery.error ? 'Type name manually' : 'No results found'
              }
              onUpdate={update}
              options={resourceOptions}
              portError={portQuery.error ? 'Error fetching ports' : undefined}
              portOptions={portOptions}
              portsLoading={portQuery.isLoading}
              portsMessage={
                portQuery.error ? 'Type port number manually' : 'No ports found'
              }
            />
          )}
        </Stack>
      </AppDialogBody>
      <AppDialogFooter>
        <DialogCancelButton onClick={onClose} />
        <Button
          bg='blue.500'
          disabled={
            Boolean(duplicateTagError) || Object.keys(errors).length > 0
          }
          height='28px'
          loading={saveMutation.isPending}
          loadingText='Saving...'
          onClick={handleSave}
          size='xs'
          _hover={{ bg: 'blue.600' }}
        >
          {isEdit ? 'Save Changes' : 'Add Config'}
        </Button>
      </AppDialogFooter>
    </AppDialog>
  )
}

export default AddConfigModal
