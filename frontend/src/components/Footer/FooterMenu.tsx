import {
  Download,
  FolderSync,
  Menu as MenuIcon,
  ScrollText,
  Server,
  Settings,
  Upload,
  Wrench,
} from 'lucide-react'

import { Box } from '@chakra-ui/react'
import { useMutation } from '@tanstack/react-query'
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

export function FooterMenu({
  onImportConfigs,
  onExportConfigs,
  onOpenAutoImport,
  onOpenServerResources,
  onOpenHelper,
  onOpenSettings,
}: FooterMenuProps) {
  const { credentials } = useGitSync()

  const openLogsMutation = useMutation({
    mutationFn: () => invoke('open_log_viewer_window_cmd'),
    meta: {
      errorToast: { title: 'Failed to open log viewer', duration: 3000 },
    },
  })

  return (
    <MenuRoot>
      <MenuTrigger asChild>
        <FooterActionButton
          data-testid='config-menu'
          aria-label='Open configuration menu'
        >
          <Box as={MenuIcon} width='12px' height='12px' />
        </FooterActionButton>
      </MenuTrigger>
      <MenuContent>
        <MenuItem
          data-testid='menu-export'
          value='export'
          onClick={onExportConfigs}
        >
          <Box as={Upload} width='12px' height='12px' />
          <Box fontSize='11px'>Export Local File</Box>
        </MenuItem>

        <MenuItem
          data-testid='menu-import'
          value='import'
          onClick={onImportConfigs}
          disabled={!!credentials}
        >
          <Box as={Download} width='12px' height='12px' />
          <Box fontSize='11px'>Import Local File</Box>
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

        <MenuItem value='log-viewer' onClick={() => openLogsMutation.mutate()}>
          <Box as={ScrollText} width='12px' height='12px' />
          <Box fontSize='11px'>Log Viewer</Box>
        </MenuItem>

        <MenuSeparator borderColor='border' my={1} />

        <MenuItem
          data-testid='settings-button'
          value='settings'
          onClick={onOpenSettings}
        >
          <Box as={Settings} width='12px' height='12px' />
          <Box fontSize='11px'>Settings</Box>
        </MenuItem>
      </MenuContent>
    </MenuRoot>
  )
}
