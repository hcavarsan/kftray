import { useMemo, useState } from 'react'
import { RefreshCw, Trash2 } from 'lucide-react'
import Select from 'react-select'

import { Box, Flex, Text } from '@chakra-ui/react'
import { useQueries, useQuery } from '@tanstack/react-query'
import { invoke } from '@tauri-apps/api/core'

import { Button } from '@/components/ui/button'
import {
  AppDialog,
  AppDialogBody,
  AppDialogFooter,
} from '@/components/ui/dialog'
import { selectStyles } from '@/components/ui/select-styles'
import { Tooltip } from '@/components/ui/tooltip'
import { configsQuery } from '@/hooks/useConfigs'

import { CleanupDialog } from './CleanupDialog'
import { ResourceList } from './ResourceList'
import type {
  CleanupMode,
  ContextOption,
  FlatResource,
  NamespaceGroup,
} from './types'
import { useResourceMutations } from './useResourceMutations'
import {
  CONTEXT_TIMEOUT_MS,
  RESOURCES_KEY,
  targetLabel,
  withTimeout,
} from './utils'

interface ServerResourcesModalProps {
  onClose: () => void
}

const ALL_CONTEXTS = '__all__'

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

export function ServerResourcesModal({ onClose }: ServerResourcesModalProps) {
  const { data: configs } = useQuery({
    ...configsQuery,
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
      const target = { context, kubeconfig: path }

      byTarget[value] ??= {
        value,
        label: targetLabel(target),
        targets: [target],
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

  const { deleteMutation, cleanupMutation, invalidateResources } =
    useResourceMutations(() => setCleanupMode(null))

  const busy = isFetching || cleanupMutation.isPending

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

        <AppDialogBody>
          <ResourceList
            hasSelection={selected !== null}
            isAll={isAll}
            isFetching={isFetching}
            error={singleError}
            resources={resources}
            deletingKey={
              deleteMutation.isPending
                ? deleteMutation.variables?.key
                : undefined
            }
            onDelete={deleteMutation.mutate}
          />
        </AppDialogBody>

        <AppDialogFooter justify='space-between'>
          <Flex gap={1}>
            {resources.length > 0 && (
              <Tooltip content='Delete all resources'>
                <Button
                  aria-label='Delete all resources'
                  size='xs'
                  variant='ghost'
                  onClick={() => setCleanupMode('all')}
                  disabled={busy}
                  height='28px'
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
            <Tooltip content='Refresh'>
              <Button
                aria-label='Refresh'
                size='xs'
                variant='ghost'
                onClick={invalidateResources}
                disabled={isFetching || !selected}
                height='28px'
                px={2}
                _hover={{ bg: 'whiteAlpha.50' }}
              >
                <Box
                  as={RefreshCw}
                  width='12px'
                  height='12px'
                  animation={isFetching ? 'spin 1s linear infinite' : undefined}
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
                height='28px'
              >
                Clean {orphaned.length} Orphaned
              </Button>
            )}
          </Flex>
        </AppDialogFooter>
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
