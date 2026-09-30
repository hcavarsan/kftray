import { useMemo, useState } from 'react'
import { Info } from 'lucide-react'
import Select from 'react-select'
import CreatableSelect from 'react-select/creatable'

import {
  Button,
  Dialog,
  Flex,
  Grid,
  HStack,
  Stack,
  Text,
} from '@chakra-ui/react'
import { useQuery } from '@tanstack/react-query'
import { invoke } from '@tauri-apps/api/core'

import { Checkbox } from '@/components/ui/checkbox'
import { AppDialog } from '@/components/ui/dialog'
import { selectStyles } from '@/components/ui/select-styles'
import { toaster } from '@/components/ui/toaster'
import { Tooltip } from '@/components/ui/tooltip'
import {
  useKubeContexts,
  useKubeNamespaces,
  useKubePodLabels,
  useKubePorts,
  useKubeServices,
} from '@/hooks/useKube'
import { errorMessage } from '@/lib/errors'
import { selectFile } from '@/lib/nativeDialog'
import type { ConfigViewResult, StoredConfig } from '@/types'

import { ExposeFields } from './ExposeFields'
import { Field, TextField } from './Field'
import { KubeTargetFields } from './KubeTargetFields'
import { ProxyFields } from './ProxyFields'
import { TagsField } from './TagsField'
import type {
  AddConfigModalProps,
  ConfigDraft,
  PortOption,
  StringOption,
} from './types'
import { tagSuggestions, trimConfigValues, validateDraft } from './utils'

const workloadTypeOptions: StringOption[] = [
  { value: 'service', label: 'Service' },
  { value: 'pod', label: 'Pod' },
  { value: 'proxy', label: 'Proxy' },
  { value: 'expose', label: 'Expose' },
]

const emptyDraft: ConfigDraft = {
  alias: '',
  auto_loopback_address: false,
  context: '',
  domain_enabled: false,
  kubeconfig: 'default',
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
    kubeconfig: draft.kubeconfig ?? 'default',
    local_address: draft.local_address ?? '127.0.0.1',
    local_port: Number(draft.local_port),
    namespace: draft.namespace ?? '',
    protocol: draft.protocol ?? 'tcp',
    remote_address: draft.remote_address ?? '',
    service: draft.service ?? '',
    tags: draft.tags,
    target: draft.target ?? '',
  }
  const remote_port = draft.remote_port ? Number(draft.remote_port) : undefined

  switch (draft.workload_type ?? 'service') {
    case 'pod':
      return { ...base, remote_port, workload_type: 'pod' }
    case 'proxy':
      return { ...base, remote_port, workload_type: 'proxy' }
    case 'expose':
      return {
        ...base,
        cert_issuer: draft.cert_issuer,
        cert_issuer_kind: draft.cert_issuer_kind,
        cert_manager_enabled: draft.cert_manager_enabled,
        exposure_type: draft.exposure_type,
        ingress_annotations: draft.ingress_annotations,
        ingress_class: draft.ingress_class,
        workload_type: 'expose',
      }
    case 'service':
      return { ...base, remote_port, workload_type: 'service' }
  }
}

function ErrorText({ error, label }: { error: unknown; label: string }) {
  return error ? (
    <Text color='red.300' fontSize='xs'>
      {label}
    </Text>
  ) : null
}

function AddConfigModal({
  initialConfig,
  isEdit,
  onClose,
  onSave,
}: AddConfigModalProps) {
  const [draft, setDraft] = useState<ConfigDraft>(() => toDraft(initialConfig))
  const [isSaving, setIsSaving] = useState(false)
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
  const kubeconfig = draft.kubeconfig ?? 'default'
  const update = (next: Partial<ConfigDraft>) =>
    setDraft(current => ({ ...current, ...next }))
  const scope = {
    kubeconfig,
    context: draft.context,
    namespace: draft.namespace,
  }
  const contextQuery = useKubeContexts(
    kubeconfig,
    contextFocused || kubeconfig !== 'default',
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

  const handleSave = async () => {
    const validationErrors = validateDraft(draft)
    if (Object.keys(validationErrors).length || duplicateTagError || isSaving) {
      return
    }
    setIsSaving(true)
    try {
      if (await onSave(toConfig(trimConfigValues(draft), initialConfig))) {
        onClose()
      }
    } finally {
      setIsSaving(false)
    }
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
      closable={false}
      headerExtra={
        <HStack gap={2}>
          <Text color='gray.400' fontSize='2xs'>
            Kubeconfig:
          </Text>
          <Tooltip content={kubeconfig} portalled>
            <Button
              bg='app.hover'
              height='20px'
              onClick={async () => {
                try {
                  const selected = await selectFile()
                  if (selected) {
                    update({ kubeconfig: selected })
                  }
                } catch (error) {
                  toaster.error({
                    description: errorMessage(error),
                    title: 'Error selecting kubeconfig',
                  })
                }
              }}
              px={2}
              size='xs'
              variant='ghost'
              _hover={{ bg: 'app.active' }}
            >
              <Text fontSize='2xs' maxW='120px' truncate>
                {kubeconfig}
              </Text>
            </Button>
          </Tooltip>
        </HStack>
      }
    >
      <Dialog.Body overflowY='auto' p={3}>
        <Stack gap={2}>
          <Grid templateColumns='repeat(2, 1fr)' gap={3}>
            <Stack gap={1.5}>
              <TextField
                error={
                  workloadType === 'expose' ? shownErrors.alias : undefined
                }
                hint={
                  workloadType === 'expose' ? (
                    <Tooltip
                      content={
                        draft.exposure_type === 'public'
                          ? 'Full domain for public access (e.g., myapp.example.com). The Kubernetes service will be named using the first part before the dot (e.g., "myapp").'
                          : `Service name in cluster (accessible as ${draft.alias || 'name'}.${draft.namespace || 'namespace'}.svc.cluster.local)`
                      }
                      portalled
                    >
                      <span
                        style={{ display: 'inline-flex', alignItems: 'center' }}
                      >
                        <Info
                          size={10}
                          color='var(--chakra-colors-app-muted)'
                        />
                      </span>
                    </Tooltip>
                  ) : undefined
                }
                label={workloadType === 'expose' ? 'Domain *' : 'Alias'}
                name='alias'
                onChange={alias => update({ alias })}
                placeholder={
                  workloadType === 'expose'
                    ? draft.exposure_type === 'public'
                      ? 'myapp.example.com'
                      : 'my-service'
                    : ''
                }
                value={draft.alias ?? ''}
              />
              {workloadType !== 'expose' && (
                <Checkbox
                  checked={draft.domain_enabled ?? false}
                  onCheckedChange={event =>
                    update({ domain_enabled: event.checked === true })
                  }
                  size='xs'
                >
                  <Text color='gray.400' fontSize='xs'>
                    Enable alias as domain
                  </Text>
                </Checkbox>
              )}
            </Stack>
            <Field error={shownErrors.context} label='Context *'>
              <Select<StringOption>
                isLoading={contextQuery.isLoading}
                onBlur={() => setContextFocused(false)}
                onChange={option =>
                  update({
                    context: option?.value ?? '',
                    namespace: '',
                    service: '',
                    target: '',
                  })
                }
                onFocus={() => setContextFocused(true)}
                options={(contextQuery.data ?? []).map(({ name }) => ({
                  label: name,
                  value: name,
                }))}
                styles={selectStyles<StringOption>()}
                value={
                  draft.context
                    ? { label: draft.context, value: draft.context }
                    : null
                }
              />
              <ErrorText
                error={contextQuery.error}
                label='Error fetching contexts'
              />
            </Field>
          </Grid>
          <Grid templateColumns='repeat(2, 1fr)' gap={3}>
            <Field error={shownErrors.workload_type} label='Workload Type'>
              <Select<StringOption>
                onChange={option =>
                  update({
                    exposure_type:
                      option?.value === 'expose'
                        ? (draft.exposure_type ?? 'cluster')
                        : draft.exposure_type,
                    protocol:
                      option?.value === 'expose' ? 'tcp' : draft.protocol,
                    workload_type:
                      option?.value === 'pod'
                        ? 'pod'
                        : option?.value === 'proxy'
                          ? 'proxy'
                          : option?.value === 'expose'
                            ? 'expose'
                            : option?.value === 'service'
                              ? 'service'
                              : undefined,
                  })
                }
                options={workloadTypeOptions}
                styles={selectStyles<StringOption>()}
                value={
                  workloadTypeOptions.find(
                    option => option.value === workloadType,
                  ) ?? null
                }
              />
            </Field>
            <Field error={shownErrors.namespace} label='Namespace *'>
              <CreatableSelect<StringOption>
                formatCreateLabel={value => `Use "${value}"`}
                isLoading={namespaceQuery.isLoading}
                noOptionsMessage={() =>
                  namespaceQuery.error
                    ? 'Type namespace name manually'
                    : 'No namespaces found'
                }
                onChange={option =>
                  update({
                    namespace: option?.value ?? '',
                    service: '',
                    target: '',
                  })
                }
                options={(namespaceQuery.data ?? []).map(({ name }) => ({
                  label: name,
                  value: name,
                }))}
                styles={selectStyles<StringOption>()}
                value={
                  draft.namespace
                    ? { label: draft.namespace, value: draft.namespace }
                    : null
                }
              />
              <ErrorText
                error={namespaceQuery.error}
                label='Error fetching namespaces'
              />
            </Field>
          </Grid>
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
      </Dialog.Body>
      <Dialog.Footer
        bg='app.panel'
        borderTop='1px solid'
        borderColor='app.subtle'
        p={2}
      >
        <Flex gap={2} justify='flex-end' width='100%'>
          <Button
            height='28px'
            onClick={onClose}
            size='xs'
            variant='ghost'
            _hover={{ bg: 'app.hover' }}
          >
            Cancel
          </Button>
          <Button
            bg='blue.500'
            disabled={
              isSaving ||
              Boolean(duplicateTagError) ||
              Object.keys(errors).length > 0
            }
            height='28px'
            onClick={handleSave}
            size='xs'
            _hover={{ bg: 'blue.600' }}
          >
            {isSaving ? 'Saving...' : isEdit ? 'Save Changes' : 'Add Config'}
          </Button>
        </Flex>
      </Dialog.Footer>
    </AppDialog>
  )
}

export default AddConfigModal
