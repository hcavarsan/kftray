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
import {
  MenuContent,
  MenuItem,
  MenuRoot,
  MenuSeparator,
  MenuTrigger,
} from '@/components/ui/menu'
import { useGitSync } from '@/contexts/GitSyncContext'

interface FooterMenuProps {
  onImportConfigs: () => void
  onExportConfigs: () => void
  onOpenAutoImport: () => void
  onOpenServerResources: () => void
  onOpenHelper: () => void
  onOpenSettings: () => void
}

const httpLogSizeQueryKey = ['http-log-size']

export function FooterMenu({
  onImportConfigs,
  onExportConfigs,
  onOpenAutoImport,
  onOpenServerResources,
  onOpenHelper,
  onOpenSettings,
}: FooterMenuProps) {
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
    meta: { errorToast: { title: 'Error clearing logs', duration: 2000 } },
  })

  return (
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

        <MenuItem value='helper' onClick={onOpenHelper}>
          <Box as={Wrench} width='12px' height='12px' />
          <Box fontSize='11px'>kftray-helper</Box>
        </MenuItem>

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
  )
}
