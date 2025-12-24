import React, { useCallback, useEffect, useState } from 'react'
import { createPortal } from 'react-dom'
import {
  Box as BoxIcon,
  Database,
  GitBranch,
  RefreshCw,
  Server,
  Trash2,
} from 'lucide-react'
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

const CleanupConfirmDialog = ({
  isOpen,
  onClose,
  onConfirm,
  orphanedCount,
  contextName,
  isLoading,
}: {
  isOpen: boolean
  onClose: () => void
  onConfirm: () => void
  orphanedCount: number
  contextName: string
  isLoading: boolean
}) => {
  if (!isOpen) {
    return null
  }

  return createPortal(
    <Box
      position='fixed'
      top={0}
      left={0}
      right={0}
      bottom={0}
      zIndex={9999}
      display='flex'
      alignItems='center'
      justifyContent='center'
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
      />
      <Box
        position='relative'
        maxWidth='340px'
        width='90vw'
        bg='#111111'
        borderRadius='lg'
        border='1px solid rgba(255, 255, 255, 0.08)'
        zIndex={10000}
      >
        <Box
          p={2}
          bg='#161616'
          borderBottom='1px solid rgba(255, 255, 255, 0.05)'
          borderTopRadius='lg'
        >
          <Text fontSize='sm' fontWeight='500' color='white'>
            Clean Orphaned
          </Text>
        </Box>

        <Box p={3}>
          <Text fontSize='xs' color='whiteAlpha.700' lineHeight='1.5'>
            Delete{' '}
            <Text as='span' fontWeight='600' color='red.400'>
              {orphanedCount}
            </Text>{' '}
            orphaned {orphanedCount === 1 ? 'resource' : 'resources'}
            {contextName === 'All Contexts'
              ? ' across all contexts'
              : ` in ${contextName}`}
            ?
          </Text>
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
              loadingText='Cleaning...'
              height='26px'
            >
              Clean
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
  const [isDeleting, setIsDeleting] = useState<string | null>(null)
  const [isCleaningAll, setIsCleaningAll] = useState(false)
  const [kubeconfig] = useState<string>('default')
  const [showConfirmDialog, setShowConfirmDialog] = useState(false)

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

    try {
      setIsLoading(true)

      if (selectedContext.value === '__all__') {
        const configs = await invoke<any[]>('get_configs_cmd')
        const uniqueContexts = Array.from(
          new Set(
            configs
              .map(config => config.context)
              .filter((ctx): ctx is string => ctx != null && ctx !== ''),
          ),
        )

        const allResources: Array<{
          context: string
          groups: NamespaceGroup[]
        }> = []

        for (const contextName of uniqueContexts) {
          try {
            const resources = await invoke<NamespaceGroup[]>(
              'list_all_kftray_resources',
              {
                contextName,
                kubeconfig: kubeconfig === 'default' ? null : kubeconfig,
              },
            )

            if (resources.length > 0) {
              allResources.push({ context: contextName, groups: resources })
            }
          } catch (error) {
            console.error(
              `Error loading resources for context ${contextName}:`,
              error,
            )
          }
        }

        const flattenedGroups: NamespaceGroup[] = []

        allResources.forEach(({ context, groups }) => {
          groups.forEach(group => {
            flattenedGroups.push({
              namespace: `${context} / ${group.namespace}`,
              resources: group.resources,
            })
          })
        })

        setNamespaceGroups(flattenedGroups)
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

  const handleCleanupOrphaned = async () => {
    if (!selectedContext) {
      return
    }

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

        let totalDeleted = 0

        for (const contextName of uniqueContexts) {
          try {
            const result = await invoke<string>(
              'cleanup_all_kftray_resources',
              {
                contextName,
                kubeconfig: kubeconfig === 'default' ? null : kubeconfig,
              },
            )
            const matches = result.match(/(\d+)/)

            if (matches) {
              totalDeleted += parseInt(matches[1], 10)
            }
          } catch (error) {
            console.error(`Error cleaning context ${contextName}:`, error)
          }
        }

        toaster.success({
          title: 'Done',
          description: `Removed ${totalDeleted} orphaned`,
          duration: 2000,
        })
      } else {
        const result = await invoke<string>('cleanup_all_kftray_resources', {
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

  const orphanedCount = flatResources.filter(r => r.is_orphaned).length

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
                {orphanedCount > 0 && !isLoading && (
                  <Flex align='center' gap={1.5} flexShrink={0}>
                    <Box
                      width='5px'
                      height='5px'
                      borderRadius='full'
                      bg='red.400'
                    />
                    <Text fontSize='xs' color='whiteAlpha.600'>
                      {orphanedCount} orphaned
                    </Text>
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
              {isLoading ? (
                <Flex
                  justify='center'
                  align='center'
                  height='100%'
                  minHeight='200px'
                >
                  <Spinner size='sm' color='blue.400' />
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
              ) : flatResources.length === 0 ? (
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
                  {flatResources.map((resource, idx) => {
                    const resourceKey = `${resource.context}-${resource.displayNamespace}-${resource.resource_type}-${resource.name}`
                    const isDeletingThis = isDeleting === resourceKey

                    return (
                      <Box
                        key={idx}
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
                </Stack>
              )}
            </Dialog.Body>

            <Dialog.Footer
              px={3}
              py={2}
              bg='#161616'
              borderTop='1px solid rgba(255, 255, 255, 0.05)'
            >
              <Flex justify='flex-end' align='center' gap={2} width='100%'>
                <Button
                  size='xs'
                  variant='ghost'
                  onClick={loadResources}
                  disabled={isLoading || !selectedContext}
                  height='26px'
                  px={2}
                  _hover={{ bg: 'whiteAlpha.50' }}
                >
                  <RefreshCw size={12} />
                </Button>

                {orphanedCount > 0 && (
                  <Button
                    size='xs'
                    colorPalette='red'
                    variant='surface'
                    onClick={() => setShowConfirmDialog(true)}
                    disabled={isLoading || isCleaningAll}
                    height='26px'
                  >
                    Clean Orphaned
                  </Button>
                )}
              </Flex>
            </Dialog.Footer>
          </Dialog.Content>
        </Dialog.Positioner>
      </Dialog.Root>

      <CleanupConfirmDialog
        isOpen={showConfirmDialog}
        onClose={() => setShowConfirmDialog(false)}
        onConfirm={handleCleanupOrphaned}
        orphanedCount={orphanedCount}
        contextName={selectedContext?.label || 'All Contexts'}
        isLoading={isCleaningAll}
      />
    </>
  )
}

export default ServerResourcesModal
