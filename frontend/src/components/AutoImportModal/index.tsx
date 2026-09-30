import { useState } from 'react'
import ReactSelect from 'react-select'

import {
  Button,
  Dialog,
  Flex,
  HStack,
  Spinner,
  Stack,
  Text,
  VStack,
} from '@chakra-ui/react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { invoke } from '@tauri-apps/api/core'

import { Checkbox } from '@/components/ui/checkbox'
import { AppDialog } from '@/components/ui/dialog'
import { selectStyles } from '@/components/ui/select-styles'
import { toaster } from '@/components/ui/toaster'
import { configsQuery } from '@/hooks/useConfigs'
import { useKubeContexts } from '@/hooks/useKube'
import { selectFile } from '@/lib/nativeDialog'
import type { Config, StringOption } from '@/types'

interface AutoImportModalProps {
  onClose: () => void
}

const DEFAULT_KUBECONFIG = 'default'

const contextSelectStyles = selectStyles<StringOption>(35)

export default function AutoImportModal({ onClose }: AutoImportModalProps) {
  const queryClient = useQueryClient()
  const [kubeConfig, setKubeConfig] = useState(DEFAULT_KUBECONFIG)
  const [selectedContext, setSelectedContext] = useState<StringOption | null>(
    null,
  )
  const [aliasAsDomain, setAliasAsDomain] = useState(false)
  const [enableAutoLoopback, setEnableAutoLoopback] = useState(false)

  const contextQuery = useKubeContexts(kubeConfig, true, {
    errorToast: { title: 'Error fetching contexts', duration: 1000 },
  })
  const contextOptions = contextQuery.data?.map(context => ({
    label: context.name,
    value: context.name,
  }))
  const isDefaultKubeconfig = kubeConfig === DEFAULT_KUBECONFIG

  const changeKubeconfig = (path: string) => {
    setKubeConfig(path)
    setSelectedContext(null)
  }

  const handleSetKubeConfig = async () => {
    try {
      const path = await selectFile()

      if (path) {
        changeKubeconfig(path)
      }
    } catch {
      changeKubeconfig(DEFAULT_KUBECONFIG)
      toaster.error({
        title: 'Error',
        description: 'Failed to select kubeconfig file.',
        duration: 1000,
      })
    }
  }

  const importMutation = useMutation({
    mutationFn: async (contextName: string) => {
      const configs = await invoke<Config[]>('get_services_with_annotations', {
        contextName,
        kubeconfigPath: kubeConfig,
      })
      const json = JSON.stringify(
        configs.map(config => ({
          ...config,
          domain_enabled: aliasAsDomain || config.domain_enabled,
          auto_loopback_address:
            enableAutoLoopback || config.auto_loopback_address,
        })),
      )

      await invoke('import_configs_cmd', { json })
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: configsQuery.queryKey })
      toaster.success({
        title: 'Success',
        description: 'Configs imported successfully.',
        duration: 1000,
      })
      onClose()
    },
    onError: () => {
      toaster.error({
        title: 'Error',
        description: 'Failed to import configs.',
        duration: 1000,
      })
    },
  })

  const handleImport = () => {
    if (!selectedContext) {
      toaster.error({
        title: 'Error',
        description: 'Please select a context.',
        duration: 1000,
      })

      return
    }

    importMutation.mutate(selectedContext.value)
  }

  const kubeconfigButtonProps = (active: boolean) => ({
    size: 'xs' as const,
    variant: active ? ('solid' as const) : ('ghost' as const),
    bg: active ? 'whiteAlpha.100' : 'transparent',
    _hover: { bg: active ? 'whiteAlpha.200' : 'whiteAlpha.50' },
    height: '22px',
  })

  return (
    <AppDialog
      title='Auto Import'
      onClose={onClose}
      maxWidth='400px'
      headerPadding={1.5}
      closable={false}
      contentProps={{ mt: 70 }}
    >
      <Dialog.Body p={3}>
        <Stack gap={4}>
          <Stack gap={1.5}>
            <Flex align='center' justify='space-between'>
              <Text fontSize='xs' color='gray.400'>
                Kubeconfig *
              </Text>
              <Flex gap={2}>
                <Button
                  {...kubeconfigButtonProps(isDefaultKubeconfig)}
                  onClick={() => changeKubeconfig(DEFAULT_KUBECONFIG)}
                >
                  <Text fontSize='xs'>Default</Text>
                </Button>
                <Button
                  {...kubeconfigButtonProps(!isDefaultKubeconfig)}
                  onClick={handleSetKubeConfig}
                >
                  <Text fontSize='xs'>Set Custom Kubeconfig</Text>
                </Button>
              </Flex>
            </Flex>

            {!isDefaultKubeconfig && (
              <Flex
                bg='app.panel'
                border='1px solid'
                borderColor='app.border'
                borderRadius='md'
                height='35px'
                align='center'
                justify='space-between'
                px={2}
                _hover={{ borderColor: 'app.borderStrong' }}
              >
                <Text
                  fontSize='xs'
                  color='gray.300'
                  truncate
                  maxW='250px'
                  title={kubeConfig}
                >
                  {kubeConfig}
                </Text>
                <Button
                  size='xs'
                  variant='ghost'
                  onClick={handleSetKubeConfig}
                  bg='whiteAlpha.50'
                  _hover={{ bg: 'whiteAlpha.100' }}
                  height='22px'
                  minW='70px'
                >
                  Browse
                </Button>
              </Flex>
            )}
          </Stack>

          <Stack gap={1.5}>
            <Text fontSize='xs' color='gray.400'>
              Context *
            </Text>
            {contextQuery.isLoading ? (
              <Flex justify='center' py={2}>
                <Spinner size='sm' color='blue.400' />
              </Flex>
            ) : (
              <ReactSelect<StringOption>
                aria-label='Context'
                options={contextOptions}
                value={selectedContext}
                onChange={setSelectedContext}
                styles={contextSelectStyles}
              />
            )}
            {contextQuery.isError && (
              <Text color='red.300' fontSize='xs'>
                Please select a valid kubeconfig file
              </Text>
            )}
          </Stack>
          <Stack>
            <Checkbox
              size='xs'
              checked={aliasAsDomain}
              onCheckedChange={e => setAliasAsDomain(e.checked === true)}
            >
              <Text fontSize='xs' color='gray.400'>
                Enable alias as domain for all configurations
              </Text>
            </Checkbox>

            <Checkbox
              size='xs'
              checked={enableAutoLoopback}
              onCheckedChange={e => setEnableAutoLoopback(e.checked === true)}
            >
              <Text fontSize='xs' color='gray.400'>
                Auto select address for all configurations
              </Text>
            </Checkbox>
          </Stack>

          <VStack align='start' gap={2.5} mt={2}>
            <Text fontSize='xs' color='gray.300'>
              Services must have:
            </Text>
            <Stack gap={1.5}>
              <Text fontSize='xs' color='gray.400'>
                • Annotation{' '}
                <Text as='span' color='blue.300' fontFamily='mono'>
                  kftray.app/enabled: true
                </Text>
              </Text>
              <Text fontSize='xs' color='gray.400'>
                • Config format:{' '}
                <Text as='span' color='blue.300' fontFamily='mono'>
                  alias-localPort-targetPort
                </Text>
              </Text>
            </Stack>
          </VStack>

          <HStack justify='flex-end' gap={2} mt={2}>
            <Button
              size='xs'
              variant='ghost'
              onClick={onClose}
              _hover={{ bg: 'whiteAlpha.50' }}
              height='28px'
            >
              Cancel
            </Button>
            <Button
              size='xs'
              bg='blue.500'
              _hover={{ bg: 'blue.600' }}
              onClick={handleImport}
              disabled={!selectedContext || importMutation.isPending}
              height='28px'
            >
              Import
            </Button>
          </HStack>
        </Stack>
      </Dialog.Body>
    </AppDialog>
  )
}
