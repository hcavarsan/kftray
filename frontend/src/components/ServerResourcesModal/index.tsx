import React, { useCallback, useEffect, useRef, useState } from 'react'
import {
  Box as BoxIcon,
  Database,
  GitBranch,
  RefreshCw,
  Server,
  Trash2,
} from 'lucide-react'
import { createPortal } from 'react-dom'
import Select, { SingleValue } from 'react-select'

import {
  Badge,
  Box,
  Dialog,
  Flex,
  HStack,
  Spinner,
  Stack,
  Text,
} from '@chakra-ui/react'
import { invoke } from '@tauri-apps/api/core'

import { Button } from '@/components/ui/button'
import { DialogCloseTrigger } from '@/components/ui/dialog'
import { toaster } from '@/components/ui/toaster'
import { Tooltip } from '@/components/ui/tooltip'
import { NamespaceGroup, ServerResource, StringOption } from '@/types'

interface ServerResourcesModalProps {
  isOpen: boolean
  onClose: () => void
}

interface FlatResource extends ServerResource {
  context: string
  displayNamespace: string
}

interface OrphanedResource {
  name: string
  context: string
  namespace: string
  resource_type: string
}

type CleanupMode = 'orphaned' | 'all'

const CONTEXT_TIMEOUT_MS = 8000

const withTimeout = <T, >(
  promise: Promise<T>,
  ms: number,
): Promise<T> => {
  return Promise.race([
    promise,
    new Promise<T>((_, reject) =>
      setTimeout(() => reject(new Error('Timeout')), ms),
    ),
  ])
}

const CleanupConfirmDialog = ({
  isOpen,
  onClose,
  onConfirm,
  resources,
  contextName,
  isLoading,
  mode,
}: {
  isOpen: boolean
  onClose: () => void
  onConfirm: () => void
  resources: OrphanedResource[]
  contextName: string
  isLoading: boolean
  mode: CleanupMode
}) => {
  if (!isOpen) {
    return null
  }

  const count = resources.length
  const title = mode === 'orphaned' ? 'Clean Orphaned Resources' : 'Delete All Resources'
  const description = mode === 'orphaned'
    ? `Delete ${count} orphaned ${count === 1 ? 'resource' : 'resources'}`
    : `Delete ${count} ${count === 1 ? 'resource' : 'resources'}`

  return createPortal(
    <Box
      position='fixed'
      top={0}
      left={0}
      right={0}
      bottom={0}
      zIndex={10001}
      display='flex'
      alignItems='center'
      justifyContent='center'
      pointerEvents='auto'
    >
      <Box
        position='fixed'
        top={0}
        left={0}
        right={0}
        bottom={0}
        bg='rgba(0, 0, 0, 0.5)'
        backdropFilter='blur(4px)'
        onClick={onClose}
        pointerEvents='auto'
      />
      <Box
        position='relative'
        maxWidth='420px'
        width='90vw'
        bg='#111111'
        borderRadius='lg'
        border='1px solid rgba(255, 255, 255, 0.08)'
        zIndex={10002}
        onClick={e => e.stopPropagation()}
        pointerEvents='auto'
      >
        <Box
          p={2}
          bg='#161616'
          borderBottom='1px solid rgba(255, 255, 255, 0.05)'
          borderTopRadius='lg'
        >
          <Text fontSize='sm' fontWeight='500' color='white'>
            {title}
          </Text>
        </Box>

        <Box p={3}>
          <Text fontSize='xs' color='whiteAlpha.700' lineHeight='1.5' mb={3}>
            {description}
            {contextName === 'All Contexts'
              ? ' across all contexts'
              : ` in ${contextName}`}
            ?
            {mode === 'all' && (
              <Text as='span' color='orange.400' fontWeight='500'>
                {' '}This will also stop active port forwards.
              </Text>
            )}
          </Text>

          {resources.length > 0 && (
            <Box
              bg='#0a0a0a'
              borderRadius='md'
              border='1px solid rgba(255, 255, 255, 0.05)'
              maxHeight='200px'
              overflowY='auto'
              css={{
                '&::-webkit-scrollbar': {
                  width: '4px',
                },
                '&::-webkit-scrollbar-track': {
                  background: 'transparent',
                },
                '&::-webkit-scrollbar-thumb': {
                  background: 'rgba(255, 255, 255, 0.15)',
                  borderRadius: '2px',
                },
              }}
            >
              {resources.map((resource, idx) => (
                <Box
                  key={`${resource.context}-${resource.namespace}-${resource.resource_type}-${resource.name}`}
                  px={2}
                  py={1.5}
                  borderBottom={
                    idx < resources.length - 1
                      ? '1px solid rgba(255, 255, 255, 0.03)'
                      : 'none'
                  }
                >
                  <Text fontSize='xs' color='whiteAlpha.800' truncate>
                    {resource.name}
                  </Text>
                  <Flex gap={1} fontSize='10px' color='whiteAlpha.500' mt={0.5}>
                    <Text>{resource.resource_type}</Text>
                    <Text color='whiteAlpha.300'>·</Text>
                    <Text truncate>{resource.context}</Text>
                    <Text color='whiteAlpha.300'>/</Text>
                    <Text truncate>{resource.namespace}</Text>
                  </Flex>
                </Box>
              ))}
            </Box>
          )}
        </Box>

        <Box
          p={2}
          borderTop='1px solid rgba(255, 255, 255, 0.05)'
          bg='#161616'
          borderBottomRadius='lg'
        >
          <HStack justify='flex-end' gap={2}>
            <Button
              size='xs'
              variant='ghost'
              onClick={onClose}
              disabled={isLoading}
              _hover={{ bg: 'whiteAlpha.50' }}
              height='26px'
            >
              Cancel
            </Button>
            <Button
              size='xs'
              colorPalette='red'
              onClick={onConfirm}
              loading={isLoading}
              loadingText='Deleting...'
              height='26px'
            >
              Delete {count}
            </Button>
          </HStack>
        </Box>
      </Box>
    </Box>,
    document.body,
  )
}


const ServerResourcesModal: React.FC<ServerResourcesModalProps> = ({
  isOpen,
  onClose,
}) => {

  const [contexts, setContexts] = useState<StringOption[]>([])
  const [selectedContext, setSelectedContext] = useState<StringOption | null>(
    null,
  )
  const [namespaceGroups, setNamespaceGroups] = useState<NamespaceGroup[]>([])
  const [isLoading, setIsLoading] = useState(false)
  const [loadingProgress, setLoadingProgress] = useState<{
    loaded: number
    total: number
  } | null>(null)
  const [isDeleting, setIsDeleting] = useState<string | null>(null)
  const [isCleaningAll, setIsCleaningAll] = useState(false)
  const [kubeconfig] = useState<string>('default')
  const [showConfirmDialog, setShowConfirmDialog] = useState(false)
  const [cleanupMode, setCleanupMode] = useState<CleanupMode>('orphaned')
  const abortControllerRef = useRef<AbortController | null>(null)

  const selectStyles = {
    control: (base: any) => ({
      ...base,
      background: '#111111',
      borderColor: 'rgba(255, 255, 255, 0.08)',
      minHeight: '28px',
      height: '28px',
      fontSize: '12px',
      boxShadow: 'none',
      cursor: 'pointer',
      '&:hover': {
        borderColor: 'rgba(255, 255, 255, 0.15)',
      },
    }),
    valueContainer: (base: any) => ({
      ...base,
      padding: '0 8px',
      height: '28px',
    }),
    menu: (base: any) => ({
      ...base,
      background: '#161616',
      border: '1px solid rgba(255, 255, 255, 0.08)',
      fontSize: '12px',
      zIndex: 99999,
    }),
    menuList: (base: any) => ({
      ...base,
      padding: 0,
      maxHeight: '150px',
    }),
    option: (base: any, state: any) => ({
      ...base,
      background: state.isFocused ? 'rgba(255, 255, 255, 0.1)' : 'transparent',
      color: 'white',
      padding: '6px 10px',
      cursor: 'pointer',
      '&:active': {
        background: 'rgba(255, 255, 255, 0.15)',
      },
    }),
    singleValue: (base: any) => ({
      ...base,
      color: 'white',
      fontSize: '12px',
    }),
    input: (base: any) => ({
      ...base,
      color: 'white',
      fontSize: '12px',
      margin: 0,
      padding: 0,
    }),
    placeholder: (base: any) => ({
      ...base,
      color: 'rgba(255, 255, 255, 0.4)',
      fontSize: '12px',
    }),
    indicatorSeparator: () => ({
      display: 'none',
    }),
    dropdownIndicator: (base: any) => ({
      ...base,
      padding: '0 6px',
    }),
    menuPortal: (base: any) => ({
      ...base,
      zIndex: 99999,
    }),
  }

  const loadContexts = async () => {
    try {
      const configs = await invoke<any[]>('get_configs_cmd')

      const uniqueContexts = Array.from(
        new Set(
          configs
            .map(config => config.context)
            .filter((ctx): ctx is string => ctx != null && ctx !== ''),
        ),
      ).sort()

      const contextOptions = [
        { value: '__all__', label: 'All Contexts' },
        ...uniqueContexts.map(ctx => ({
          value: ctx,
          label: ctx,
        })),
      ]

      setContexts(contextOptions)

      return contextOptions
    } catch (error) {
      console.error('Error loading contexts:', error)
      toaster.error({
        title: 'Error',
        description: 'Failed to load contexts',
        duration: 3000,
      })

      return []
    }
  }

  const loadResources = useCallback(async () => {
    if (!selectedContext) {
      return
    }

    if (abortControllerRef.current) {
      abortControllerRef.current.abort()
    }
    abortControllerRef.current = new AbortController()

    try {
      setIsLoading(true)
      setLoadingProgress(null)

      if (selectedContext.value === '__all__') {
        const configs = await invoke<any[]>('get_configs_cmd')
        const uniqueContexts = Array.from(
          new Set(
            configs
              .map(config => config.context)
              .filter((ctx): ctx is string => ctx != null && ctx !== ''),
          ),
        )

        setLoadingProgress({ loaded: 0, total: uniqueContexts.length })

        const allGroups: NamespaceGroup[] = []
        let loadedCount = 0

        await Promise.all(
          uniqueContexts.map(async contextName => {
            try {
              const resources = await withTimeout(
                invoke<NamespaceGroup[]>('list_all_kftray_resources', {
                  contextName,
                  kubeconfig: kubeconfig === 'default' ? null : kubeconfig,
                }),
                CONTEXT_TIMEOUT_MS,
              )

              if (resources.length > 0) {
                resources.forEach(group => {
                  allGroups.push({
                    namespace: `${contextName} / ${group.namespace}`,
                    resources: group.resources,
                  })
                })
              }
            } catch (error) {
              console.warn(`Skipped context ${contextName}: ${error}`)
            } finally {
              loadedCount++
              setLoadingProgress({ loaded: loadedCount, total: uniqueContexts.length })
              setNamespaceGroups([...allGroups])
            }
          }),
        )

        setNamespaceGroups(allGroups)
      } else {
        const resources = await invoke<NamespaceGroup[]>(
          'list_all_kftray_resources',
          {
            contextName: selectedContext.value,
            kubeconfig: kubeconfig === 'default' ? null : kubeconfig,
          },
        )

        setNamespaceGroups(resources)
      }
    } catch (error) {
      console.error('Error loading resources:', error)
      toaster.error({
        title: 'Error',
        description: 'Failed to load resources',
        duration: 3000,
      })
    } finally {
      setIsLoading(false)
      setLoadingProgress(null)
    }
  }, [selectedContext, kubeconfig])

  useEffect(() => {
    if (isOpen) {
      setNamespaceGroups([])
      loadContexts().then(contextOptions => {
        if (contextOptions.length > 1) {
          setSelectedContext(contextOptions[1])
        }
      })
    } else {
      setSelectedContext(null)
      setNamespaceGroups([])
      if (abortControllerRef.current) {
        abortControllerRef.current.abort()
      }
    }
  }, [isOpen])

  useEffect(() => {
    if (selectedContext) {
      loadResources()
    } else {
      setNamespaceGroups([])
    }
  }, [selectedContext, loadResources])

  const handleDeleteResource = async (resource: FlatResource) => {
    const resourceKey = `${resource.context}-${resource.displayNamespace}-${resource.resource_type}-${resource.name}`

    try {
      setIsDeleting(resourceKey)

      const contextToUse =
        selectedContext?.value === '__all__'
          ? resource.context
          : selectedContext?.value

      await invoke('delete_kftray_resource', {
        contextName: contextToUse,
        namespace: resource.namespace,
        resourceType: resource.resource_type,
        resourceName: resource.name,
        configId: resource.config_id,
        kubeconfig: kubeconfig === 'default' ? null : kubeconfig,
      })

      toaster.success({
        title: 'Deleted',
        description: `Removed ${resource.name}`,
        duration: 2000,
      })

      await loadResources()
    } catch (error) {
      console.error('Error deleting resource:', error)
      toaster.error({
        title: 'Error',
        description: `Failed to delete: ${error}`,
        duration: 3000,
      })
    } finally {
      setIsDeleting(null)
    }
  }

  const handleCleanup = async () => {
    if (!selectedContext) {
      return
    }

    const command = cleanupMode === 'orphaned'
      ? 'cleanup_orphaned_kftray_resources'
      : 'cleanup_all_kftray_resources'

    try {
      setIsCleaningAll(true)

      if (selectedContext.value === '__all__') {
        const configs = await invoke<any[]>('get_configs_cmd')
        const uniqueContexts = Array.from(
          new Set(
            configs
              .map(config => config.context)
              .filter((ctx): ctx is string => ctx != null && ctx !== ''),
          ),
        )

        const results = await Promise.allSettled(
          uniqueContexts.map(async contextName => {
            const result = await withTimeout(
              invoke<string>(command, {
                contextName,
                kubeconfig: kubeconfig === 'default' ? null : kubeconfig,
              }),
              CONTEXT_TIMEOUT_MS * 2,
            )
            const matches = result.match(/(\d+)/)


            
return matches ? parseInt(matches[1], 10) : 0
          }),
        )

        const totalDeleted = results
          .filter(
            (result): result is PromiseFulfilledResult<number> =>
              result.status === 'fulfilled',
          )
          .reduce((sum, result) => sum + result.value, 0)

        toaster.success({
          title: 'Done',
          description: `Removed ${totalDeleted} resources`,
          duration: 2000,
        })
      } else {
        const result = await invoke<string>(command, {
          contextName: selectedContext.value,
          kubeconfig: kubeconfig === 'default' ? null : kubeconfig,
        })

        toaster.success({
          title: 'Done',
          description: result,
          duration: 2000,
        })
      }

      setShowConfirmDialog(false)
      await loadResources()
    } catch (error) {
      console.error('Error cleaning up:', error)
      toaster.error({
        title: 'Error',
        description: `Cleanup failed: ${error}`,
        duration: 3000,
      })
    } finally {
      setIsCleaningAll(false)
    }
  }

  const getResourceIcon = (type: string) => {
    switch (type) {
      case 'pod':
        return <BoxIcon size={12} />
      case 'deployment':
        return <Server size={12} />
      case 'service':
        return <GitBranch size={12} />
      case 'ingress':
        return <Database size={12} />
      default:
        return <Server size={12} />
    }
  }

  const flatResources: FlatResource[] = namespaceGroups.flatMap(group => {
    const parts = group.namespace.split(' / ')
    const hasContext = parts.length === 2
    const context = hasContext ? parts[0] : selectedContext?.value || ''
    const displayNamespace = hasContext ? parts[1] : group.namespace

    return group.resources.map(resource => ({
      ...resource,
      context,
      displayNamespace,
    }))
  })

  const orphanedResources: OrphanedResource[] = flatResources
    .filter(r => r.is_orphaned)
    .map(r => ({
      name: r.name,
      context: r.context,
      namespace: r.displayNamespace,
      resource_type: r.resource_type,
    }))

  const allResourcesForDialog: OrphanedResource[] = flatResources.map(r => ({
    name: r.name,
    context: r.context,
    namespace: r.displayNamespace,
    resource_type: r.resource_type,
  }))

  const orphanedCount = orphanedResources.length
  const totalCount = flatResources.length

  const openCleanupDialog = (mode: CleanupMode) => {
    setCleanupMode(mode)
    setShowConfirmDialog(true)
  }

  return (
    <>
      <Dialog.Root
        open={isOpen}
        onOpenChange={({ open }) => !open && onClose()}
        modal={true}
      >
        <Dialog.Backdrop
          bg='transparent'
          backdropFilter='blur(4px)'
          height='100vh'
        />
        <Dialog.Positioner overflow='hidden'>
          <Dialog.Content
            onClick={e => e.stopPropagation()}
            maxWidth='500px'
            width='90vw'
            height='92vh'
            bg='#111111'
            border='1px solid rgba(255, 255, 255, 0.08)'
            borderRadius='lg'
            overflow='hidden'
            position='absolute'
            my={2}
          >
            <DialogCloseTrigger
              style={{
                marginTop: '-4px',
              }}
            />

            <Dialog.Header
              p={3}
              bg='#161616'
              borderBottom='1px solid rgba(255, 255, 255, 0.05)'
            >
              <Text fontSize='sm' fontWeight='medium' color='gray.100'>
                Server Resources
              </Text>
            </Dialog.Header>

            <Box
              px={3}
              py={2}
              borderBottom='1px solid rgba(255, 255, 255, 0.05)'
              bg='#111111'
            >
              <Flex align='center' gap={3}>
                <Box flex='1'>
                  <Select
                    value={selectedContext}
                    onChange={(option: SingleValue<StringOption>) => {
                      setSelectedContext(option)
                    }}
                    options={contexts}
                    styles={selectStyles}
                    placeholder='Select context...'
                    isSearchable={true}
                    menuPlacement='auto'
                  />
                </Box>
                {loadingProgress && (
                  <Text fontSize='10px' color='whiteAlpha.500' flexShrink={0}>
                    {loadingProgress.loaded}/{loadingProgress.total}
                  </Text>
                )}
                {!isLoading && totalCount > 0 && (
                  <Flex align='center' gap={2} flexShrink={0}>
                    <Text fontSize='xs' color='whiteAlpha.500'>
                      {totalCount}
                    </Text>
                    {orphanedCount > 0 && (
                      <Flex align='center' gap={1}>
                        <Box
                          width='5px'
                          height='5px'
                          borderRadius='full'
                          bg='red.400'
                        />
                        <Text fontSize='xs' color='red.400'>
                          {orphanedCount}
                        </Text>
                      </Flex>
                    )}
                  </Flex>
                )}
              </Flex>
            </Box>

            <Dialog.Body
              p={3}
              flex='1'
              overflowY='auto'
              css={{
                '&::-webkit-scrollbar': {
                  width: '5px',
                },
                '&::-webkit-scrollbar-track': {
                  background: 'transparent',
                },
                '&::-webkit-scrollbar-thumb': {
                  background: 'rgba(255, 255, 255, 0.15)',
                  borderRadius: '3px',
                },
                '&::-webkit-scrollbar-thumb:hover': {
                  background: 'rgba(255, 255, 255, 0.25)',
                },
              }}
            >
              {isLoading && flatResources.length === 0 ? (
                <Flex
                  justify='center'
                  align='center'
                  height='100%'
                  minHeight='200px'
                  direction='column'
                  gap={2}
                >
                  <Spinner size='sm' color='blue.400' />
                  {loadingProgress && (
                    <Text fontSize='xs' color='whiteAlpha.500'>
                      Loading contexts...
                    </Text>
                  )}
                </Flex>
              ) : !selectedContext ? (
                <Flex
                  direction='column'
                  align='center'
                  justify='center'
                  height='100%'
                  minHeight='200px'
                >
                  <Text fontSize='xs' color='whiteAlpha.400'>
                    Select a context
                  </Text>
                </Flex>
              ) : flatResources.length === 0 && !isLoading ? (
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
              ) : (
                <Stack gap={2}>
                  {flatResources.map((resource) => {
                    const resourceKey = `${resource.context}-${resource.displayNamespace}-${resource.resource_type}-${resource.name}`
                    const isDeletingThis = isDeleting === resourceKey

                    return (
                      <Box
                        key={resourceKey}
                        bg='#161616'
                        p={2}
                        borderRadius='md'
                        border='1px solid rgba(255, 255, 255, 0.04)'
                        _hover={{
                          borderColor: 'rgba(255, 255, 255, 0.08)',
                        }}
                      >
                        <Flex align='center' gap={2} mb={1.5}>
                          <Box color='whiteAlpha.500' flexShrink={0}>
                            {getResourceIcon(resource.resource_type)}
                          </Box>

                          <Tooltip
                            content={resource.name}
                            portalled
                            positioning={{ placement: 'top' }}
                          >
                            <Text
                              fontSize='xs'
                              fontWeight='500'
                              color='white'
                              flex='1'
                              truncate
                              cursor='default'
                            >
                              {resource.name}
                            </Text>
                          </Tooltip>

                          <Badge
                            size='xs'
                            colorPalette={resource.is_orphaned ? 'red' : 'gray'}
                            variant='subtle'
                            flexShrink={0}
                          >
                            {resource.is_orphaned ? 'orphaned' : 'active'}
                          </Badge>

                          <Button
                            size='2xs'
                            variant='ghost'
                            onClick={() => handleDeleteResource(resource)}
                            disabled={isDeletingThis}
                            flexShrink={0}
                            px={1}
                            opacity={0.5}
                            _hover={{ opacity: 1, color: 'red.400' }}
                          >
                            {isDeletingThis ? (
                              <Spinner size='xs' />
                            ) : (
                              <Trash2 size={11} />
                            )}
                          </Button>
                        </Flex>

                        <Flex
                          align='center'
                          gap={1.5}
                          fontSize='xs'
                          color='whiteAlpha.500'
                        >
                          <Tooltip
                            content={resource.context}
                            portalled
                            positioning={{ placement: 'top' }}
                          >
                            <Text
                              truncate
                              maxWidth='120px'
                              cursor='default'
                            >
                              {resource.context}
                            </Text>
                          </Tooltip>

                          <Text color='whiteAlpha.300'>/</Text>

                          <Tooltip
                            content={resource.displayNamespace}
                            portalled
                            positioning={{ placement: 'top' }}
                          >
                            <Text
                              truncate
                              maxWidth='100px'
                              cursor='default'
                            >
                              {resource.displayNamespace}
                            </Text>
                          </Tooltip>

                          <Text color='whiteAlpha.300' flexShrink={0}>
                            ·
                          </Text>

                          <Text flexShrink={0}>{resource.resource_type}</Text>

                          <Text color='whiteAlpha.300' flexShrink={0}>
                            ·
                          </Text>

                          <Text flexShrink={0}>{resource.age}</Text>
                        </Flex>
                      </Box>
                    )
                  })}
                  {isLoading && flatResources.length > 0 && (
                    <Flex justify='center' py={2}>
                      <Spinner size='xs' color='blue.400' />
                    </Flex>
                  )}
                </Stack>
              )}
            </Dialog.Body>

            <Dialog.Footer
              px={3}
              py={2}
              bg='#161616'
              borderTop='1px solid rgba(255, 255, 255, 0.05)'
            >
              <Flex justify='space-between' align='center' width='100%'>
                <Flex gap={1}>
                  {totalCount > 0 && (
                    <Tooltip content='Delete all resources' portalled>
                      <Button
                        size='xs'
                        variant='ghost'
                        onClick={() => openCleanupDialog('all')}
                        disabled={isLoading || isCleaningAll}
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
                      size='xs'
                      variant='ghost'
                      onClick={loadResources}
                      disabled={isLoading || !selectedContext}
                      height='26px'
                      px={2}
                      _hover={{ bg: 'whiteAlpha.50' }}
                    >
                      <RefreshCw size={12} className={isLoading ? 'animate-spin' : ''} />
                    </Button>
                  </Tooltip>

                  {orphanedCount > 0 && (
                    <Button
                      size='xs'
                      colorPalette='red'
                      variant='surface'
                      onClick={() => openCleanupDialog('orphaned')}
                      disabled={isLoading || isCleaningAll}
                      height='26px'
                    >
                      Clean {orphanedCount} Orphaned
                    </Button>
                  )}
                </Flex>
              </Flex>
            </Dialog.Footer>
          </Dialog.Content>
        </Dialog.Positioner>
      </Dialog.Root>

      <CleanupConfirmDialog
        isOpen={showConfirmDialog}
        onClose={() => setShowConfirmDialog(false)}
        onConfirm={handleCleanup}
        resources={cleanupMode === 'orphaned' ? orphanedResources : allResourcesForDialog}
        contextName={selectedContext?.label || 'All Contexts'}
        isLoading={isCleaningAll}
        mode={cleanupMode}
      />
    </>
  )
}

export default ServerResourcesModal
