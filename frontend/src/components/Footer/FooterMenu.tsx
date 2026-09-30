import { useState } from 'react'
import {
  Download,
  Eraser,
  FolderSync,
  Menu as MenuIcon,
  Server,
  Settings,
  Upload,
  Wrench,
} from 'lucide-react'

import { Box } from '@chakra-ui/react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { invoke } from '@tauri-apps/api/core'

import { FooterActionButton } from '@/components/Footer/FooterActionButton'
import type {
  HelperAction,
  HelperActionResult,
} from '@/components/Footer/HelperResultDialog'
import { HelperResultDialog } from '@/components/Footer/HelperResultDialog'
import {
  MenuContent,
  MenuItem,
  MenuRoot,
  MenuSeparator,
  MenuTrigger,
  MenuTriggerItem,
} from '@/components/ui/menu'
import { useGitSync } from '@/contexts/GitSyncContext'
import { errorMessage } from '@/lib/errors'

interface FooterMenuProps {
  onImportConfigs: () => void
  onExportConfigs: () => void
  onOpenAutoImport: () => void
  onOpenServerResources: () => void
  onOpenSettings: () => void
}

const httpLogSizeQueryKey = ['http-log-size']

const helperActions = {
  install: {
    command: 'install_helper',
    message: 'kftray-helper was successfully installed',
  },
  uninstall: {
    command: 'remove_helper',
    message: 'kftray-helper was successfully uninstalled',
  },
} as const

function useHelperMutation(
  action: HelperAction,
  onResult: (result: HelperActionResult) => void,
) {
  const { command, message } = helperActions[action]

  return useMutation({
    mutationFn: () => invoke<boolean>(command),
    onSuccess: result => {
      if (result) {
        onResult({ success: true, message, action })
      }
    },
    onError: error => {
      onResult({ success: false, message: errorMessage(error), action })
    },
  })
}

export function FooterMenu({
  onImportConfigs,
  onExportConfigs,
  onOpenAutoImport,
  onOpenServerResources,
  onOpenSettings,
}: FooterMenuProps) {
  const { credentials } = useGitSync()
  const queryClient = useQueryClient()
  const [helperActionResult, setHelperActionResult] =
    useState<HelperActionResult | null>(null)

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
    meta: { errorToast: { title: 'Error clearing logs', duration: 2000 } },
  })

  const installHelperMutation = useHelperMutation(
    'install',
    setHelperActionResult,
  )
  const uninstallHelperMutation = useHelperMutation(
    'uninstall',
    setHelperActionResult,
  )

  return (
    <>
      {helperActionResult && (
        <HelperResultDialog
          result={helperActionResult}
          onClose={() => setHelperActionResult(null)}
        />
      )}
      <MenuRoot>
        <MenuTrigger asChild>
          <FooterActionButton
            aria-label='Open configuration menu'
            onClick={() => refetchLogSize()}
          >
            <Box as={MenuIcon} width='12px' height='12px' />
          </FooterActionButton>
        </MenuTrigger>
        <MenuContent>
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
              <MenuItem
                value='install-helper'
                onClick={() => installHelperMutation.mutate()}
              >
                <Box as={Download} width='12px' height='12px' />
                <Box fontSize='11px'>Install kftray-helper</Box>
              </MenuItem>

              <MenuItem
                value='uninstall-helper'
                onClick={() => uninstallHelperMutation.mutate()}
              >
                <Box as={Download} width='12px' height='12px' />
                <Box fontSize='11px'>Uninstall kftray-helper</Box>
              </MenuItem>
            </MenuContent>
          </MenuRoot>

          <MenuItem value='server-resources' onClick={onOpenServerResources}>
            <Box as={Server} width='12px' height='12px' />
            <Box fontSize='11px'>Server Resources</Box>
          </MenuItem>

          <MenuSeparator borderColor='border' my={1} />

          <MenuItem value='settings' onClick={onOpenSettings}>
            <Box as={Settings} width='12px' height='12px' />
            <Box fontSize='11px'>Settings</Box>
          </MenuItem>
        </MenuContent>
      </MenuRoot>
    </>
  )
}
