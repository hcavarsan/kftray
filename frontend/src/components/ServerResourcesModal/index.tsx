import React, { useCallback, useEffect, useState } from 'react'
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
  Spinner,
  Stack,
  Text,
} from '@chakra-ui/react'
import { invoke } from '@tauri-apps/api/core'

import { Button } from '@/components/ui/button'
import { DialogCloseTrigger } from '@/components/ui/dialog'
import { toaster } from '@/components/ui/toaster'
import { NamespaceGroup, ServerResource, StringOption } from '@/types'

interface ServerResourcesModalProps {
  isOpen: boolean
  onClose: () => void
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
  const [hasLoadedOnce, setHasLoadedOnce] = useState(false)

  const selectStyles = {
    control: (base: any) => ({
      ...base,
      background: '#111111',
      borderColor: 'rgba(255, 255, 255, 0.08)',
      minHeight: '32px',
      height: '32px',
      fontSize: '12px',
      boxShadow: 'none',
      cursor: 'pointer',
      display: 'flex',
      alignItems: 'center',
      '&:hover': {
        borderColor: 'rgba(255, 255, 255, 0.15)',
      },
    }),
    valueContainer: (base: any) => ({
      ...base,
      padding: '0 8px',
      display: 'flex',
      alignItems: 'center',
      height: '32px',
    }),
    menu: (base: any) => ({
      ...base,
      background: '#161616',
      border: '1px solid rgba(255, 255, 255, 0.08)',
      fontSize: '12px',
      zIndex: 99999,
      position: 'absolute',
    }),
    menuList: (base: any) => ({
      ...base,
      padding: 0,
    }),
    option: (base: any, state: any) => ({
      ...base,
      background: state.isFocused ? 'rgba(255, 255, 255, 0.1)' : 'transparent',
      color: 'white',
      padding: '8px 12px',
      cursor: 'pointer',
      '&:active': {
        background: 'rgba(255, 255, 255, 0.15)',
      },
    }),
    singleValue: (base: any) => ({
      ...base,
      color: 'white',
      fontSize: '12px',
      margin: 0,
      lineHeight: '32px',
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
      color: 'rgba(255, 255, 255, 0.5)',
      fontSize: '12px',
      margin: 0,
      lineHeight: '32px',
    }),
    indicatorSeparator: () => ({
      display: 'none',
    }),
    dropdownIndicator: (base: any) => ({
      ...base,
      padding: '0 8px',
      display: 'flex',
      alignItems: 'center',
    }),
    menuPortal: (base: any) => ({
      ...base,
      zIndex: 99999,
    }),
  }

  useEffect(() => {
    if (isOpen) {
      loadContexts()
      setSelectedContext(null)
      setNamespaceGroups([])
      setHasLoadedOnce(false)
    }
  }, [isOpen])

  useEffect(() => {
    if (selectedContext && hasLoadedOnce) {
      loadResources()
    } else if (!selectedContext) {
      setNamespaceGroups([])
    }
  }, [selectedContext, hasLoadedOnce, loadResources])

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
    } catch (error) {
      console.error('Error loading contexts:', error)
      toaster.error({
        title: 'Error',
        description: 'Failed to load contexts from configs',
        duration: 3000,
      })
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
        description: 'Failed to load kftray-server resources',
        duration: 3000,
      })
    } finally {
      setIsLoading(false)
    }
  }, [selectedContext, kubeconfig])

  const handleDeleteResource = async (resource: ServerResource) => {
    const resourceKey = `${resource.namespace}-${resource.resource_type}-${resource.name}`

    try {
      setIsDeleting(resourceKey)

      await invoke('delete_kftray_resource', {
        contextName: selectedContext?.value,
        namespace: resource.namespace,
        resourceType: resource.resource_type,
        resourceName: resource.name,
        configId: resource.config_id,
        kubeconfig: kubeconfig === 'default' ? null : kubeconfig,
      })

      toaster.success({
        title: 'Resource Deleted',
        description: `Deleted ${resource.resource_type} ${resource.name}`,
        duration: 3000,
      })

      await loadResources()
    } catch (error) {
      console.error('Error deleting resource:', error)
      toaster.error({
        title: 'Error',
        description: `Failed to delete ${resource.resource_type}: ${error}`,
        duration: 5000,
      })
    } finally {
      setIsDeleting(null)
    }
  }

  const handleCleanupAll = async () => {
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
            console.error(
              `Error cleaning up resources for context ${contextName}:`,
              error,
            )
          }
        }

        toaster.success({
          title: 'Cleanup Complete',
          description: `Successfully deleted ${totalDeleted} resources across all contexts`,
          duration: 5000,
        })
      } else {
        const result = await invoke<string>('cleanup_all_kftray_resources', {
          contextName: selectedContext.value,
          kubeconfig: kubeconfig === 'default' ? null : kubeconfig,
        })

        toaster.success({
          title: 'Cleanup Complete',
          description: result,
          duration: 5000,
        })
      }

      await loadResources()
    } catch (error) {
      console.error('Error cleaning up resources:', error)
      toaster.error({
        title: 'Error',
        description: `Failed to cleanup resources: ${error}`,
        duration: 5000,
      })
    } finally {
      setIsCleaningAll(false)
    }
  }

  const getResourceIcon = (type: string) => {
    switch (type) {
      case 'pod':
        return <BoxIcon size={14} />
      case 'deployment':
        return <Server size={14} />
      case 'service':
        return <GitBranch size={14} />
      case 'ingress':
        return <Database size={14} />
      default:
        return <Server size={14} />
    }
  }

  const totalResources = namespaceGroups.reduce(
    (sum, group) => sum + group.resources.length,
    0,
  )
  const orphanedCount = namespaceGroups.reduce(
    (sum, group) => sum + group.resources.filter(r => r.is_orphaned).length,
    0,
  )

  return (
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
      <Dialog.Positioner>
        <Dialog.Content
          onClick={e => e.stopPropagation()}
          maxWidth='700px'
          width='90vw'
          height='92vh'
          bg='#111111'
          border='1px solid rgba(255, 255, 255, 0.08)'
          borderRadius='lg'
          position='relative'
          my={2}
        >
          <Dialog.Header
            p={3}
            bg='#161616'
            borderBottom='1px solid rgba(255, 255, 255, 0.05)'
            position='relative'
          >
            <DialogCloseTrigger position='absolute' top='10px' right='10px' />
            <Flex align='center' gap={2} pr={8}>
              <Server size={15} color='rgba(255, 255, 255, 0.5)' />
              <Box>
                <Text fontSize='sm' fontWeight='500' color='white'>
                  KFtray Server Resources
                </Text>
              </Box>
            </Flex>
          </Dialog.Header>

          <Dialog.Body
            p={3}
            overflowY='auto'
            overflowX='visible'
            css={{
              '&::-webkit-scrollbar': {
                width: '6px',
              },
              '&::-webkit-scrollbar-track': {
                background: 'transparent',
              },
              '&::-webkit-scrollbar-thumb': {
                background: 'rgba(255, 255, 255, 0.2)',
                borderRadius: '3px',
              },
              '&::-webkit-scrollbar-thumb:hover': {
                background: 'rgba(255, 255, 255, 0.3)',
              },
            }}
          >
            <Stack gap={2.5}>
              <Box
                bg='#161616'
                p={2.5}
                borderRadius='md'
                border='1px solid rgba(255, 255, 255, 0.08)'
              >
                <Text
                  fontSize='xs'
                  fontWeight='500'
                  color='whiteAlpha.700'
                  mb={1.5}
                >
                  Context
                </Text>
                <Box position='relative' zIndex={10}>
                  <Select
                    value={selectedContext}
                    onChange={(option: SingleValue<StringOption>) => {
                      setSelectedContext(option)
                      setHasLoadedOnce(true)
                    }}
                    options={contexts}
                    styles={selectStyles}
                    placeholder='Select a context...'
                    isSearchable={true}
                    menuPlacement='auto'
                  />
                </Box>

                {totalResources > 0 && (
                  <Flex
                    align='center'
                    gap={3}
                    mt={3}
                    pt={2.5}
                    borderTop='1px solid rgba(255, 255, 255, 0.05)'
                  >
                    <Flex align='center' gap={1.5}>
                      <Box
                        width='6px'
                        height='6px'
                        borderRadius='full'
                        bg='blue.400'
                      />
                      <Text fontSize='xs' color='whiteAlpha.600'>
                        {totalResources} total
                      </Text>
                    </Flex>
                    {orphanedCount > 0 && (
                      <Flex align='center' gap={1.5}>
                        <Box
                          width='6px'
                          height='6px'
                          borderRadius='full'
                          bg='red.400'
                        />
                        <Text fontSize='xs' color='whiteAlpha.600'>
                          {orphanedCount} orphaned
                        </Text>
                      </Flex>
                    )}
                    <Flex gap={2} ml='auto'>
                      <Button
                        size='xs'
                        variant='outline'
                        onClick={loadResources}
                        disabled={isLoading}
                      >
                        <RefreshCw size={12} />
                      </Button>
                      <Button
                        size='xs'
                        colorPalette='red'
                        variant='surface'
                        onClick={handleCleanupAll}
                        disabled={isCleaningAll || isLoading}
                      >
                        {isCleaningAll ? (
                          <Spinner size='xs' />
                        ) : (
                          <>
                            <Trash2 size={12} />
                            Clean All
                          </>
                        )}
                      </Button>
                    </Flex>
                  </Flex>
                )}
              </Box>

              {isLoading ? (
                <Flex justify='center' align='center' minHeight='300px'>
                  <Stack align='center' gap={3}>
                    <Spinner size='lg' color='blue.400' />
                    <Text fontSize='sm' color='whiteAlpha.600'>
                      Loading resources...
                    </Text>
                  </Stack>
                </Flex>
              ) : !selectedContext ? (
                <Box
                  bg='#161616'
                  p={6}
                  borderRadius='md'
                  border='1px solid rgba(255, 255, 255, 0.08)'
                  textAlign='center'
                >
                  <Server
                    size={40}
                    color='rgba(255, 255, 255, 0.2)'
                    style={{ margin: '0 auto 12px' }}
                  />
                  <Text fontSize='sm' fontWeight='500' color='white' mb={1}>
                    Select a Context
                  </Text>
                  <Text fontSize='xs' color='whiteAlpha.500' lineHeight='1.5'>
                    Choose a context from the dropdown above to view
                    <br />
                    server resources deployed in that cluster
                  </Text>
                </Box>
              ) : namespaceGroups.length === 0 ? (
                <Box
                  bg='#161616'
                  p={6}
                  borderRadius='md'
                  border='1px solid rgba(255, 255, 255, 0.08)'
                  textAlign='center'
                >
                  <Server
                    size={40}
                    color='rgba(255, 255, 255, 0.2)'
                    style={{ margin: '0 auto 12px' }}
                  />
                  <Text fontSize='sm' fontWeight='500' color='white' mb={1}>
                    No Resources Found
                  </Text>
                  <Text fontSize='xs' color='whiteAlpha.500' lineHeight='1.5'>
                    Server resources will appear here when proxy or expose
                    <br />
                    workloads are deployed to this context
                  </Text>
                </Box>
              ) : (
                namespaceGroups.map(group => {
                  const parts = group.namespace.split(' / ')
                  const hasContext = parts.length === 2
                  const contextName = hasContext ? parts[0] : null
                  const namespaceName = hasContext ? parts[1] : group.namespace

                  return (
                    <Box key={group.namespace}>
                      <Flex align='center' gap={2} mb={1.5} px={1}>
                        <Box
                          width='3px'
                          height='14px'
                          bg='blue.400'
                          borderRadius='full'
                        />
                        {hasContext && (
                          <>
                            <Text
                              fontSize='xs'
                              fontWeight='600'
                              color='purple.400'
                              letterSpacing='wide'
                            >
                              {contextName}
                            </Text>
                            <Text fontSize='xs' color='whiteAlpha.400'>
                              /
                            </Text>
                          </>
                        )}
                        <Text
                          fontSize='xs'
                          fontWeight='600'
                          color='whiteAlpha.800'
                          letterSpacing='wide'
                        >
                          {namespaceName}
                        </Text>
                        <Badge size='xs' colorPalette='gray' variant='subtle'>
                          {group.resources.length}
                        </Badge>
                      </Flex>

                      <Stack gap={1}>
                        {group.resources.map((resource, idx) => {
                          const resourceKey = `${resource.namespace}-${resource.resource_type}-${resource.name}`
                          const isDeletingThis = isDeleting === resourceKey

                          return (
                            <Box
                              key={idx}
                              bg='#161616'
                              p={2}
                              borderRadius='md'
                              border='1px solid rgba(255, 255, 255, 0.06)'
                              transition='all 0.15s ease'
                              _hover={{
                                borderColor: 'rgba(255, 255, 255, 0.12)',
                                bg: '#181818',
                              }}
                            >
                              <Flex align='center' gap={2.5}>
                                <Box color='whiteAlpha.600' flexShrink={0}>
                                  {getResourceIcon(resource.resource_type)}
                                </Box>

                                <Box flex='1' minWidth='0'>
                                  <Flex align='center' gap={2} mb={0.5}>
                                    <Text
                                      fontSize='xs'
                                      fontWeight='500'
                                      color='white'
                                      letterSpacing='tight'
                                    >
                                      {resource.name}
                                    </Text>
                                    <Badge
                                      size='xs'
                                      colorPalette={
                                        resource.is_orphaned ? 'red' : 'green'
                                      }
                                      variant='subtle'
                                    >
                                      {resource.is_orphaned
                                        ? 'orphaned'
                                        : 'active'}
                                    </Badge>
                                  </Flex>

                                  <Flex align='center' gap={2} flexWrap='wrap'>
                                    <Text fontSize='xs' color='whiteAlpha.500'>
                                      {resource.resource_type}
                                    </Text>
                                    <Text fontSize='xs' color='whiteAlpha.400'>
                                      •
                                    </Text>
                                    <Text fontSize='xs' color='whiteAlpha.500'>
                                      {resource.age}
                                    </Text>
                                    {resource.status && (
                                      <>
                                        <Text
                                          fontSize='xs'
                                          color='whiteAlpha.400'
                                        >
                                          •
                                        </Text>
                                        <Text
                                          fontSize='xs'
                                          color='whiteAlpha.500'
                                          truncate
                                        >
                                          {resource.status}
                                        </Text>
                                      </>
                                    )}
                                  </Flex>
                                </Box>

                                <Button
                                  size='xs'
                                  variant='ghost'
                                  colorPalette='red'
                                  onClick={() => handleDeleteResource(resource)}
                                  disabled={isDeletingThis}
                                  flexShrink={0}
                                  px={2}
                                >
                                  {isDeletingThis ? (
                                    <Spinner size='xs' />
                                  ) : (
                                    <Trash2 size={13} />
                                  )}
                                </Button>
                              </Flex>
                            </Box>
                          )
                        })}
                      </Stack>
                    </Box>
                  )
                })
              )}
            </Stack>
          </Dialog.Body>
        </Dialog.Content>
      </Dialog.Positioner>
    </Dialog.Root>
  )
}

export default ServerResourcesModal
