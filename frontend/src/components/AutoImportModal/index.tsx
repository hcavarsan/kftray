import { type ChangeEvent, useState } from 'react'
import ReactSelect from 'react-select'

import {
  Dialog,
  Flex,
  Input,
  Spinner,
  Stack,
  Text,
  VStack,
} from '@chakra-ui/react'
import { useMutation, useQueryClient } from '@tanstack/react-query'

import { allHttpLogsEnabledQueryKey } from '@/components/PortForwardTable/GroupAccordion/PortForwardRow/useHttpLogsEnabled'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import {
  AppDialog,
  AppDialogFooter,
  DialogCancelButton,
} from '@/components/ui/dialog'
import { selectStyles } from '@/components/ui/select-styles'
import { toaster } from '@/components/ui/toaster'
import { configsQuery } from '@/hooks/useConfigs'
import { useKubeContexts } from '@/hooks/useKube'
import {
  DEFAULT_KUBECONFIG,
  useKubeconfigPicker,
} from '@/hooks/useKubeconfigPicker'
import type { StringOption } from '@/types'

import { importAnnotatedServices } from './importAnnotatedServices'

interface AutoImportModalProps {
  onClose: () => void
}

const contextSelectStyles = selectStyles<StringOption>()

export function AutoImportModal({ onClose }: AutoImportModalProps) {
  const queryClient = useQueryClient()
  const [kubeConfig, setKubeConfig] = useState(DEFAULT_KUBECONFIG)
  const [selectedContext, setSelectedContext] = useState<StringOption | null>(
    null,
  )
  const [namespace, setNamespace] = useState('')
  const [aliasAsDomain, setAliasAsDomain] = useState(false)
  const [enableAutoLoopback, setEnableAutoLoopback] = useState(false)

  const contextQuery = useKubeContexts(kubeConfig, true, {
    errorToast: { title: 'Error fetching contexts', duration: 1000 },
  })
  const contextOptions = contextQuery.data?.map(context => ({
    label: context.name,
    value: context.name,
  }))

  const changeKubeconfig = (path: string) => {
    setKubeConfig(path)
    setSelectedContext(null)
  }

  const {
    browse: handleSetKubeConfig,
    browseFolder: handleSetKubeConfigFolder,
    isDefault: isDefaultKubeconfig,
    resetToDefault,
  } = useKubeconfigPicker({
    value: kubeConfig,
    onChange: changeKubeconfig,
    onError: () => changeKubeconfig(DEFAULT_KUBECONFIG),
    meta: {
      errorToast: {
        title: 'Error',
        description: 'Failed to select kubeconfig.',
        duration: 1000,
      },
    },
  })

  const importMutation = useMutation({
    mutationFn: (contextName: string) =>
      importAnnotatedServices({
        contextName,
        kubeconfigPath: kubeConfig,
        namespace,
        aliasAsDomain,
        autoLoopback: enableAutoLoopback,
      }),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: configsQuery.queryKey })
      queryClient.invalidateQueries({ queryKey: allHttpLogsEnabledQueryKey })
      toaster.success({
        title: 'Success',
        description: 'Configs imported successfully.',
        duration: 1000,
      })
      onClose()
    },
    meta: {
      errorToast: {
        title: 'Failed to import configs',
        duration: 5000,
      },
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
    bg: active ? 'bg.hover' : 'transparent',
    _hover: { bg: active ? 'bg.active' : 'bg.faint' },
    height: '22px',
  })

  return (
    <AppDialog title='Auto Import' onClose={onClose} maxWidth='400px'>
      <Dialog.Body p={3}>
        <Stack gap={4}>
          <Stack gap={1.5}>
            <Flex align='center' justify='space-between'>
              <Text fontSize='xs' color='fg.muted'>
                Kubeconfig *
              </Text>
              <Flex gap={2}>
                <Button
                  {...kubeconfigButtonProps(isDefaultKubeconfig)}
                  onClick={resetToDefault}
                >
                  <Text fontSize='xs'>Default</Text>
                </Button>
                <Button
                  {...kubeconfigButtonProps(false)}
                  onClick={handleSetKubeConfig}
                >
                  <Text fontSize='xs'>File</Text>
                </Button>
                <Button
                  {...kubeconfigButtonProps(false)}
                  onClick={handleSetKubeConfigFolder}
                >
                  <Text fontSize='xs'>Folder</Text>
                </Button>
              </Flex>
            </Flex>

            {!isDefaultKubeconfig && (
              <Flex
                bg='bg.surface'
                border='1px solid'
                borderColor='border'
                borderRadius='md'
                height='35px'
                align='center'
                justify='space-between'
                px={2}
                _hover={{ borderColor: 'border.emphasized' }}
              >
                <Text
                  fontSize='xs'
                  color='fg.secondary'
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
                  bg='bg.faint'
                  _hover={{ bg: 'bg.hover' }}
                  height='22px'
                  minW='70px'
                >
                  Browse
                </Button>
              </Flex>
            )}
          </Stack>

          <Stack gap={1.5}>
            <Text fontSize='xs' color='fg.muted'>
              Context *
            </Text>
            {contextQuery.isLoading ? (
              <Flex justify='center' py={2}>
                <Spinner size='sm' color='accent.fg' />
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
              <Text color='danger.fg' fontSize='xs'>
                Please select a valid kubeconfig file or folder
              </Text>
            )}
          </Stack>
          <Stack gap={1.5}>
            <Text fontSize='xs' color='fg.muted'>
              Namespace
            </Text>
            <Input
              data-testid='auto-import-namespace'
              aria-label='Namespace'
              value={namespace}
              onChange={(e: ChangeEvent<HTMLInputElement>) =>
                setNamespace(e.target.value)
              }
              placeholder='All namespaces'
              bg='bg.surface'
              borderColor='border'
              _hover={{ borderColor: 'border.strong' }}
              height='30px'
              fontSize='12px'
              borderRadius='md'
              px={2}
            />
            <Text fontSize='xs' color='fg.subtle'>
              Leave empty to check every namespace you can list.
            </Text>
          </Stack>
          <Stack>
            <Checkbox
              size='xs'
              checked={aliasAsDomain}
              onCheckedChange={e => setAliasAsDomain(e.checked === true)}
            >
              <Text fontSize='xs' color='fg.muted'>
                Enable alias as domain for all configurations
              </Text>
            </Checkbox>

            <Checkbox
              size='xs'
              checked={enableAutoLoopback}
              onCheckedChange={e => setEnableAutoLoopback(e.checked === true)}
            >
              <Text fontSize='xs' color='fg.muted'>
                Auto select address for all configurations
              </Text>
            </Checkbox>
          </Stack>

          <VStack align='start' gap={2.5} mt={2}>
            <Text fontSize='xs' color='fg.secondary'>
              Services must have:
            </Text>
            <Stack gap={1.5}>
              <Text fontSize='xs' color='fg.muted'>
                • Annotation{' '}
                <Text as='span' color='accent.fg' fontFamily='mono'>
                  kftray.app/enabled: true
                </Text>
              </Text>
              <Text fontSize='xs' color='fg.muted'>
                • Config format:{' '}
                <Text as='span' color='accent.fg' fontFamily='mono'>
                  alias-localPort-targetPort
                </Text>
              </Text>
            </Stack>
          </VStack>
        </Stack>
      </Dialog.Body>
      <AppDialogFooter>
        <DialogCancelButton onClick={onClose} />
        <Button
          size='xs'
          bg='accent.solid'
          _hover={{ bg: 'accent.solidHover' }}
          onClick={handleImport}
          disabled={!selectedContext || importMutation.isPending}
          height='28px'
        >
          Import
        </Button>
      </AppDialogFooter>
    </AppDialog>
  )
}
