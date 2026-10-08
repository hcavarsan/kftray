import { GitBranch, Keyboard, Plus, Settings } from 'lucide-react'

import { Box, Group } from '@chakra-ui/react'

import { BulkDeleteButton } from '@/components/Footer/BulkDeleteButton'
import { FooterActionButton } from '@/components/Footer/FooterActionButton'
import { FooterMenu } from '@/components/Footer/FooterMenu'
import { SyncConfigsButton } from '@/components/Footer/SyncConfigsButton'
import { Tooltip } from '@/components/ui/tooltip'
import { useGitSync } from '@/contexts/GitSyncContext'
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
  onOpenHelper: () => void
  onOpenServerResources: () => void
}

export function Footer({
  selectedConfigs,
  deleteConfigs,
  onAddConfig,
  onImportConfigs,
  onExportConfigs,
  onOpenGitSync,
  onOpenAutoImport,
  onOpenShortcuts,
  onOpenSettings,
  onOpenHelper,
  onOpenServerResources,
}: FooterProps) {
  const { credentials } = useGitSync()

  return (
    <Box
      display='flex'
      alignItems='center'
      justifyContent='space-between'
      width='100%'
      bg='bg.surface'
      px={3}
      py={2}
      borderRadius='lg'
      border='1px solid'
      borderColor='border'
      position='relative'
      mt='-1px'
      height='50px'
    >
      <Group display='flex' alignItems='center' gap={2}>
        <FooterMenu
          onImportConfigs={onImportConfigs}
          onExportConfigs={onExportConfigs}
          onOpenAutoImport={onOpenAutoImport}
          onOpenServerResources={onOpenServerResources}
          onOpenSettings={onOpenSettings}
          onOpenHelper={onOpenHelper}
        />

        <Tooltip
          content='Add New Config'
          positioning={{
            placement: 'top-end',
            offset: { mainAxis: 8, crossAxis: 0 },
          }}
        >
          <FooterActionButton
            data-testid='add-config-button'
            aria-label='Add new config'
            onClick={onAddConfig}
            disabled={!!credentials}
          >
            <Box as={Plus} width='12px' height='12px' />
          </FooterActionButton>
        </Tooltip>

        <BulkDeleteButton
          selectedConfigs={selectedConfigs}
          deleteConfigs={deleteConfigs}
        />
      </Group>

      <Group display='flex' alignItems='center' gap={2}>
        <Tooltip
          content='Manage Global Shortcuts'
          positioning={{
            placement: 'top-end',
            offset: { mainAxis: 8, crossAxis: 0 },
          }}
        >
          <FooterActionButton
            aria-label='Manage global shortcuts'
            onClick={onOpenShortcuts}
          >
            <Box as={Keyboard} width='14px' height='14px' />
          </FooterActionButton>
        </Tooltip>

        <Tooltip
          content='Configure Git Sync'
          positioning={{
            placement: 'top-end',
            offset: { mainAxis: 8, crossAxis: 0 },
          }}
        >
          <FooterActionButton
            aria-label='Configure git sync'
            onClick={onOpenGitSync}
          >
            <Box display='flex' alignItems='center' gap={1}>
              <Box as={GitBranch} width='14px' height='14px' />
              <Box as={Settings} width='14px' height='14px' />
            </Box>
          </FooterActionButton>
        </Tooltip>
        <SyncConfigsButton />
      </Group>
    </Box>
  )
}
