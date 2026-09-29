import { useState } from 'react'
import {
  Download,
  Eraser,
  FolderSync,
  GitBranch,
  Keyboard,
  Menu as MenuIcon,
  Plus,
  Server,
  Settings,
  Upload,
  Wrench,
} from 'lucide-react'

import { Box, Dialog, Group } from '@chakra-ui/react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { invoke } from '@tauri-apps/api/core'

import BulkDeleteButton from '@/components/Footer/BulkDeleteButton'
import SyncConfigsButton from '@/components/Footer/SyncConfigsButton'
import { Button } from '@/components/ui/button'
import { AppDialog } from '@/components/ui/dialog'
import {
  MenuContent,
  MenuItem,
  MenuRoot,
  MenuSeparator,
  MenuTrigger,
  MenuTriggerItem,
} from '@/components/ui/menu'
import { toaster } from '@/components/ui/toaster'
import { Tooltip } from '@/components/ui/tooltip'
import { useGitSync } from '@/contexts/GitSyncContext'
import { errorMessage } from '@/lib/errors'
import type { Config } from '@/types'

interface FooterProps {
  selectedConfigs: Config[]
  deleteConfigs: (ids: number[]) => Promise<boolean>
  onAddConfig: () => void
  onImportConfigs: () => void
  onExportConfigs: () => void
  onOpenGitSync: () => void
  onOpenAutoImport: () => void
  onOpenShortcuts: () => void
  onOpenSettings: () => void
  onOpenServerResources: () => void
}

const httpLogSizeQueryKey = ['http-log-size']

function Footer({
  selectedConfigs,
  deleteConfigs,
  onAddConfig,
  onImportConfigs,
  onExportConfigs,
  onOpenGitSync,
  onOpenAutoImport,
  onOpenShortcuts,
  onOpenSettings,
  onOpenServerResources,
}: FooterProps) {
  const { credentials } = useGitSync()
  const queryClient = useQueryClient()

  const {
    data: logSize = 0,
    refetch: refetchLogSize,
    isError: hasLogSizeError,
  } = useQuery({
    queryKey: httpLogSizeQueryKey,
    queryFn: () => invoke<number>('get_http_log_size'),
  })

  const clearLogsMutation = useMutation({
    mutationFn: () => invoke('clear_http_logs'),
    onSuccess: () => {
      queryClient.setQueryData(httpLogSizeQueryKey, 0)
    },
    onError: error => {
      toaster.error({
        title: 'Error clearing logs',
        description: errorMessage(error),
        duration: 2000,
      })
    },
  })

  const [helperActionResult, setHelperActionResult] = useState<{
    success: boolean
    message: string
    action: 'install' | 'uninstall'
  } | null>(null)

  const handleInstallHelper = async () => {
    try {
      const result = await invoke<boolean>('install_helper')

      if (result) {
        setHelperActionResult({
          success: true,
          message: 'kftray-helper was successfully installed',
          action: 'install',
        })
      }
    } catch (error) {
      setHelperActionResult({
        success: false,
        message: errorMessage(error),
        action: 'install',
      })
    }
  }

  const handleUninstallHelper = async () => {
    try {
      const result = await invoke<boolean>('remove_helper')

      if (result) {
        setHelperActionResult({
          success: true,
          message: 'kftray-helper was successfully uninstalled',
          action: 'uninstall',
        })
      }
    } catch (error) {
      setHelperActionResult({
        success: false,
        message: errorMessage(error),
        action: 'uninstall',
      })
    }
  }

  const closeHelperActionDialog = () => {
    setHelperActionResult(null)
  }

  const renderMenuItems = () => (
    <>
      <MenuItem value='export' onClick={onExportConfigs}>
        <Box as={Upload} width='12px' height='12px' />
        <Box fontSize='11px'>Export Local File</Box>
      </MenuItem>

      <MenuItem
        value='import'
        onClick={onImportConfigs}
        disabled={!!credentials}
      >
        <Box as={Download} width='12px' height='12px' />
        <Box fontSize='11px'>Import Local File</Box>
      </MenuItem>

      <MenuItem
        value='clear-logs'
        onClick={() => clearLogsMutation.mutate()}
        disabled={
          logSize === 0 || hasLogSizeError || clearLogsMutation.isPending
        }
      >
        <Box as={Eraser} width='12px' height='12px' />
        <Box fontSize='11px'>
          Prune Logs ({(logSize / (1024 * 1024)).toFixed(2)} MB)
        </Box>
      </MenuItem>

      <MenuItem value='auto-import' onClick={onOpenAutoImport}>
        <Box as={FolderSync} width='12px' height='12px' />
        <Box fontSize='11px'>Auto Import</Box>
      </MenuItem>

      <MenuRoot>
        <MenuTriggerItem>
          <Box as={Wrench} width='12px' height='12px' />
          <Box fontSize='11px'>Helper</Box>
        </MenuTriggerItem>
        <MenuContent>
          <MenuItem value='install-helper' onClick={handleInstallHelper}>
            <Box as={Download} width='12px' height='12px' />
            <Box fontSize='11px'>Install kftray-helper</Box>
          </MenuItem>

          <MenuItem value='uninstall-helper' onClick={handleUninstallHelper}>
            <Box as={Download} width='12px' height='12px' />
            <Box fontSize='11px'>Uninstall kftray-helper</Box>
          </MenuItem>
        </MenuContent>
      </MenuRoot>

      <MenuItem value='server-resources' onClick={onOpenServerResources}>
        <Box as={Server} width='12px' height='12px' />
        <Box fontSize='11px'>Server Resources</Box>
      </MenuItem>

      <MenuSeparator borderColor='app.border' my={1} />

      <MenuItem value='settings' onClick={onOpenSettings}>
        <Box as={Settings} width='12px' height='12px' />
        <Box fontSize='11px'>Settings</Box>
      </MenuItem>
    </>
  )

  return (
    <>
      {helperActionResult && (
        <AppDialog
          title={
            helperActionResult.success
              ? helperActionResult.action === 'install'
                ? 'Installation Successful'
                : 'Uninstallation Successful'
              : helperActionResult.action === 'install'
                ? 'Installation Failed'
                : 'Uninstallation Failed'
          }
          onClose={closeHelperActionDialog}
          maxWidth='400px'
        >
          <Dialog.Body p={3}>
            <Box
              p={3}
              bg={
                helperActionResult.success
                  ? 'rgba(56, 161, 105, 0.1)'
                  : 'rgba(229, 62, 62, 0.1)'
              }
              borderRadius='md'
              border={`1px solid ${helperActionResult.success ? 'rgba(56, 161, 105, 0.2)' : 'rgba(229, 62, 62, 0.2)'}`}
            >
              <Box
                fontSize='xs'
                color={helperActionResult.success ? 'green.300' : 'red.300'}
              >
                {helperActionResult.message}
              </Box>
            </Box>

            <Box display='flex' justifyContent='flex-end' mt={4}>
              <Button
                onClick={closeHelperActionDialog}
                size='xs'
                bg={helperActionResult.success ? 'green.600' : 'red.600'}
                _hover={{
                  bg: helperActionResult.success ? 'green.700' : 'red.700',
                }}
                height='28px'
              >
                Close
              </Button>
            </Box>
          </Dialog.Body>
        </AppDialog>
      )}
      <Box
        display='flex'
        alignItems='center'
        justifyContent='space-between'
        width='100%'
        bg='app.panel'
        px={3}
        py={2}
        borderRadius='lg'
        border='1px solid'
        borderColor='app.border'
        position='relative'
        mt='-1px'
        height='50px'
      >
        <Group display='flex' alignItems='center' gap={2}>
          <MenuRoot>
            <MenuTrigger asChild>
              <Button
                aria-label='Open configuration menu'
                size='sm'
                variant='ghost'
                onClick={() => refetchLogSize()}
                height='32px'
                minWidth='32px'
                bg='whiteAlpha.50'
                px={1.5}
                borderRadius='md'
                border='1px solid'
                borderColor='app.border'
                _hover={{ bg: 'whiteAlpha.100' }}
              >
                <Box as={MenuIcon} width='12px' height='12px' />
              </Button>
            </MenuTrigger>
            <MenuContent>{renderMenuItems()}</MenuContent>
          </MenuRoot>

          <Tooltip
            content='Add New Config'
            portalled
            positioning={{
              strategy: 'absolute',
              placement: 'top-end',
              offset: { mainAxis: 8, crossAxis: 0 },
            }}
          >
            <Button
              aria-label='Add new config'
              size='sm'
              variant='ghost'
              onClick={onAddConfig}
              disabled={!!credentials}
              height='32px'
              minWidth='32px'
              bg='whiteAlpha.50'
              px={1.5}
              borderRadius='md'
              border='1px solid'
              borderColor='app.border'
              _hover={{ bg: 'whiteAlpha.100' }}
            >
              <Box as={Plus} width='12px' height='12px' />
            </Button>
          </Tooltip>

          <BulkDeleteButton
            selectedConfigs={selectedConfigs}
            deleteConfigs={deleteConfigs}
          />
        </Group>

        <Group display='flex' alignItems='center' gap={2}>
          <Tooltip
            content='Manage Global Shortcuts'
            portalled
            positioning={{
              strategy: 'absolute',
              placement: 'top-end',
              offset: { mainAxis: 8, crossAxis: 0 },
            }}
          >
            <Button
              aria-label='Manage global shortcuts'
              size='sm'
              variant='ghost'
              onClick={onOpenShortcuts}
              height='32px'
              minWidth='32px'
              bg='whiteAlpha.50'
              px={1.5}
              borderRadius='md'
              border='1px solid'
              borderColor='app.border'
              _hover={{ bg: 'whiteAlpha.100' }}
            >
              <Box as={Keyboard} width='14px' height='14px' />
            </Button>
          </Tooltip>

          <Tooltip
            content='Configure Git Sync'
            portalled
            positioning={{
              strategy: 'absolute',
              placement: 'top-end',
              offset: { mainAxis: 8, crossAxis: 0 },
            }}
          >
            <Button
              aria-label='Configure git sync'
              size='sm'
              variant='ghost'
              onClick={onOpenGitSync}
              height='32px'
              minWidth='32px'
              bg='whiteAlpha.50'
              px={1.5}
              borderRadius='md'
              border='1px solid'
              borderColor='app.border'
              _hover={{ bg: 'whiteAlpha.100' }}
            >
              <Box display='flex' alignItems='center' gap={1}>
                <Box as={GitBranch} width='14px' height='14px' />
                <Box as={Settings} width='14px' height='14px' />
              </Box>
            </Button>
          </Tooltip>
          <SyncConfigsButton />
        </Group>
      </Box>
    </>
  )
}

export default Footer
