import { useMemo, useState } from 'react'
import { RefreshCw, Trash2 } from 'lucide-react'
import Select from 'react-select'

import { Box, Dialog, Flex, Spinner, Stack, Text } from '@chakra-ui/react'
import {
  useMutation,
  useQueries,
  useQuery,
  useQueryClient,
} from '@tanstack/react-query'
import { invoke } from '@tauri-apps/api/core'

import { Button } from '@/components/ui/button'
import { AppDialog } from '@/components/ui/dialog'
import { selectStyles } from '@/components/ui/select-styles'
import { toaster } from '@/components/ui/toaster'
import { Tooltip } from '@/components/ui/tooltip'
import { fetchConfigsWithState } from '@/hooks/useConfigs'
import { errorMessage } from '@/lib/errors'

import { CleanupDialog } from './CleanupDialog'
import { ResourceRow } from './ResourceRow'
import type {
  CleanupMode,
  ContextOption,
  ContextTarget,
  FlatResource,
  NamespaceGroup,
} from './types'

interface ServerResourcesModalProps {
  onClose: () => void
}

const ALL_CONTEXTS = '__all__'
const CONTEXT_TIMEOUT_MS = 8000
const RESOURCES_KEY = 'server-resources'

const baseContextStyles = selectStyles<ContextOption>(28)

const contextSelectStyles: typeof baseContextStyles = {
  ...baseContextStyles,
  control: (base, state) => ({
    ...baseContextStyles.control?.(base, state),
    background: 'var(--chakra-colors-app-bg)',
    cursor: 'pointer',
  }),
  menuList: base => ({ ...base, padding: 0, maxHeight: '150px' }),
  option: (base, state) => ({
    ...baseContextStyles.option?.(base, state),
    color: 'white',
    padding: '6px 10px',
    cursor: 'pointer',
  }),
  placeholder: (base, state) => ({
    ...baseContextStyles.placeholder?.(base, state),
    color: 'var(--chakra-colors-app-text-disabled)',
  }),
  indicatorSeparator: () => ({ display: 'none' }),
  dropdownIndicator: base => ({ ...base, padding: '0 6px' }),
}

const withTimeout = <T,>(promise: Promise<T>, ms: number): Promise<T> => {
  let timer = 0

  return Promise.race([
    promise,
    new Promise<T>((_, reject) => {
      timer = window.setTimeout(() => reject(new Error('Timeout')), ms)
    }),
  ]).finally(() => clearTimeout(timer))
}

const bodyScrollbar = {
  '&::-webkit-scrollbar': { width: '5px' },
  '&::-webkit-scrollbar-track': { background: 'transparent' },
  '&::-webkit-scrollbar-thumb': {
    background: 'var(--chakra-colors-app-border-strong)',
    borderRadius: '3px',
  },
  '&::-webkit-scrollbar-thumb:hover': {
    background:
      'color-mix(in srgb, var(--chakra-colors-white) 25%, transparent)',
  },
}

export default function ServerResourcesModal({
  onClose,
}: ServerResourcesModalProps) {
  const queryClient = useQueryClient()
  const { data: configs } = useQuery({
    queryKey: ['configs', 'server-resources'],
    queryFn: fetchConfigsWithState,
    meta: {
      errorToast: {
        title: 'Error',
        description: 'Failed to load contexts',
        duration: 3000,
      },
    },
  })
  const [selectedValue, setSelectedValue] = useState<string | null>(null)
  const [cleanupMode, setCleanupMode] = useState<CleanupMode | null>(null)

  const contextOptions = useMemo<ContextOption[]>(() => {
    const byTarget: Record<string, ContextOption> = {}

    for (const { context, kubeconfig } of configs ?? []) {
      if (!context) {
        continue
      }
      const path = kubeconfig && kubeconfig !== 'default' ? kubeconfig : null
      const value = `${path ?? ''}\n${context}`

      byTarget[value] ??= {
        value,
        label: path ? `${context} (${path.split('/').pop()})` : context,
        targets: [{ context, kubeconfig: path }],
      }
    }
    const options = Object.values(byTarget).sort((a, b) =>
      a.label.localeCompare(b.label),
    )

    return [
      {
        value: ALL_CONTEXTS,
        label: 'All Contexts',
        targets: options.flatMap(option => option.targets),
      },
      ...options,
    ]
  }, [configs])

  const defaultValue = contextOptions[1]?.value

  if (selectedValue === null && defaultValue) {
    setSelectedValue(defaultValue)
  }

  const selected =
    contextOptions.find(option => option.value === selectedValue) ?? null
  const isAll = selected?.value === ALL_CONTEXTS
  const targets = selected?.targets ?? []

  const resourceQueries = useQueries({
    queries: targets.map(target => ({
      queryKey: [RESOURCES_KEY, target.context, target.kubeconfig],
      queryFn: () => {
        const request = invoke<NamespaceGroup[]>('list_all_kftray_resources', {
          contextName: target.context,
          kubeconfig: target.kubeconfig,
        })

        return isAll ? withTimeout(request, CONTEXT_TIMEOUT_MS) : request
      },
      retry: false,
      meta: isAll
        ? undefined
        : {
            errorToast: {
              title: 'Error',
              description: 'Failed to load resources',
              duration: 3000,
            },
          },
    })),
  })

  const isFetching = resourceQueries.some(query => query.isFetching)
  const settledCount = resourceQueries.filter(query => !query.isPending).length
  const singleError = !isAll ? resourceQueries[0]?.error : null

  const resources: FlatResource[] = resourceQueries.flatMap((query, index) => {
    const target = targets[index]

    return (query.data ?? []).flatMap(group =>
      group.resources.map(resource => ({
        ...resource,
        ...target,
        key: `${target.kubeconfig ?? ''}/${target.context}/${resource.namespace}/${resource.resource_type}/${resource.name}`,
      })),
    )
  })
  const orphaned = resources.filter(resource => resource.is_orphaned)

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
    }) => {
      const command =
        mode === 'orphaned'
          ? 'cleanup_orphaned_kftray_resources'
          : 'cleanup_all_kftray_resources'
      const run = ({ context, kubeconfig }: ContextTarget) =>
        invoke<string>(command, { contextName: context, kubeconfig })

      if (targets.length === 1) {
        return run(targets[0])
      }
      const results = await Promise.allSettled(
        targets.map(target => withTimeout(run(target), CONTEXT_TIMEOUT_MS * 2)),
      )
      const removed = results.reduce((sum, result) => {
        const count =
          result.status === 'fulfilled' ? result.value.match(/\d+/) : null

        return sum + (count ? Number(count[0]) : 0)
      }, 0)

      return `Removed ${removed} resources`
    },
    onSuccess: description => {
      toaster.success({ title: 'Done', description, duration: 2000 })
      setCleanupMode(null)
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

  const busy = isFetching || cleanupMutation.isPending
  const renderBody = () => {
    if (!selected) {
      return (
        <Flex align='center' justify='center' height='100%' minHeight='200px'>
          <Text fontSize='xs' color='whiteAlpha.400'>
            Select a context
          </Text>
        </Flex>
      )
    }
    if (resources.length === 0 && isFetching) {
      return (
        <Flex
          justify='center'
          align='center'
          height='100%'
          minHeight='200px'
          direction='column'
          gap={2}
        >
          <Spinner size='sm' color='blue.400' />
          {isAll && (
            <Text fontSize='xs' color='whiteAlpha.500'>
              Loading contexts...
            </Text>
          )}
        </Flex>
      )
    }
    if (singleError) {
      return (
        <Flex align='center' justify='center' height='100%' minHeight='200px'>
          <Text fontSize='xs' color='red.300' textAlign='center'>
            Failed to load resources: {errorMessage(singleError)}
          </Text>
        </Flex>
      )
    }
    if (resources.length === 0) {
      return (
        <Flex
          direction='column'
          align='center'
          justify='center'
          height='100%'
          minHeight='200px'
        >
          <Text fontSize='xs' color='whiteAlpha.500' mb={1}>
            No resources
          </Text>
          <Text fontSize='xs' color='whiteAlpha.400'>
            Server pods appear when port forwards start
          </Text>
        </Flex>
      )
    }

    return (
      <Stack gap={2}>
        {resources.map(resource => (
          <ResourceRow
            key={resource.key}
            resource={resource}
            isDeleting={
              deleteMutation.isPending &&
              deleteMutation.variables?.key === resource.key
            }
            onDelete={deleteMutation.mutate}
          />
        ))}
        {isFetching && (
          <Flex justify='center' py={2}>
            <Spinner size='xs' color='blue.400' />
          </Flex>
        )}
      </Stack>
    )
  }

  return (
    <>
      <AppDialog
        title='Server Resources'
        onClose={onClose}
        maxWidth='500px'
        height='92vh'
      >
        <Box
          px={3}
          py={2}
          borderBottom='1px solid'
          borderColor='app.hover'
          bg='app.bg'
        >
          <Flex align='center' gap={3}>
            <Box flex='1'>
              <Select<ContextOption>
                aria-label='Context'
                value={selected}
                onChange={option => setSelectedValue(option?.value ?? null)}
                options={contextOptions}
                styles={contextSelectStyles}
                placeholder='Select context...'
                menuPlacement='auto'
              />
            </Box>
            {isAll && isFetching && (
              <Text fontSize='10px' color='whiteAlpha.500' flexShrink={0}>
                {settledCount}/{targets.length}
              </Text>
            )}
            {!isFetching && resources.length > 0 && (
              <Flex align='center' gap={2} flexShrink={0}>
                <Text fontSize='xs' color='whiteAlpha.500'>
                  {resources.length}
                </Text>
                {orphaned.length > 0 && (
                  <Flex align='center' gap={1}>
                    <Box
                      width='5px'
                      height='5px'
                      borderRadius='full'
                      bg='red.400'
                    />
                    <Text fontSize='xs' color='red.400'>
                      {orphaned.length}
                    </Text>
                  </Flex>
                )}
              </Flex>
            )}
          </Flex>
        </Box>

        <Dialog.Body p={3} flex='1' overflowY='auto' css={bodyScrollbar}>
          {renderBody()}
        </Dialog.Body>

        <Dialog.Footer
          px={3}
          py={2}
          bg='app.panel'
          borderTop='1px solid'
          borderColor='app.hover'
        >
          <Flex justify='space-between' align='center' width='100%'>
            <Flex gap={1}>
              {resources.length > 0 && (
                <Tooltip content='Delete all resources' portalled>
                  <Button
                    aria-label='Delete all resources'
                    size='xs'
                    variant='ghost'
                    onClick={() => setCleanupMode('all')}
                    disabled={busy}
                    height='26px'
                    px={2}
                    color='whiteAlpha.600'
                    _hover={{ bg: 'whiteAlpha.50', color: 'red.400' }}
                  >
                    <Trash2 size={12} />
                  </Button>
                </Tooltip>
              )}
            </Flex>

            <Flex gap={2}>
              <Tooltip content='Refresh' portalled>
                <Button
                  aria-label='Refresh'
                  size='xs'
                  variant='ghost'
                  onClick={invalidateResources}
                  disabled={isFetching || !selected}
                  height='26px'
                  px={2}
                  _hover={{ bg: 'whiteAlpha.50' }}
                >
                  <Box
                    as={RefreshCw}
                    width='12px'
                    height='12px'
                    animation={
                      isFetching ? 'spin 1s linear infinite' : undefined
                    }
                  />
                </Button>
              </Tooltip>

              {orphaned.length > 0 && (
                <Button
                  size='xs'
                  colorPalette='red'
                  variant='surface'
                  onClick={() => setCleanupMode('orphaned')}
                  disabled={busy}
                  height='26px'
                >
                  Clean {orphaned.length} Orphaned
                </Button>
              )}
            </Flex>
          </Flex>
        </Dialog.Footer>
      </AppDialog>

      {cleanupMode && (
        <CleanupDialog
          mode={cleanupMode}
          resources={cleanupMode === 'orphaned' ? orphaned : resources}
          scope={isAll ? 'across all contexts' : `in ${selected?.label}`}
          isPending={cleanupMutation.isPending}
          onClose={() => setCleanupMode(null)}
          onConfirm={() =>
            cleanupMutation.mutate({ mode: cleanupMode, targets })
          }
        />
      )}
    </>
  )
}
