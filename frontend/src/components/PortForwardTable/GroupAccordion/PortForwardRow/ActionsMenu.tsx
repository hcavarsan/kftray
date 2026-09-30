import {
  Copy,
  FileIcon,
  Menu,
  Pencil,
  SettingsIcon,
  Trash2,
} from 'lucide-react'

import { Box, IconButton, Text } from '@chakra-ui/react'

import {
  MenuContent,
  MenuItem,
  MenuRoot,
  MenuTrigger,
} from '@/components/ui/menu'
import type { Protocol } from '@/types'

interface ActionsMenuProps {
  protocol: Protocol | undefined
  isPending: boolean
  httpLogsEnabled: boolean | undefined
  onEdit: () => void
  onDuplicate: () => void
  onOpenDeleteDialog: () => void
  onToggleHttpLogs: () => void
  onInspectLogs: () => void
  onOpenHttpLogsConfig: () => void
}

export function ActionsMenu({
  protocol,
  isPending,
  httpLogsEnabled,
  onEdit,
  onDuplicate,
  onOpenDeleteDialog,
  onToggleHttpLogs,
  onInspectLogs,
  onOpenHttpLogsConfig,
}: ActionsMenuProps) {
  return (
    <MenuRoot>
      <MenuTrigger asChild>
        <IconButton
          size='xs'
          ml={5}
          variant='ghost'
          aria-label='Actions'
          className='icon-button'
        >
          <Box as={Menu} width='12px' height='12px' />
        </IconButton>
      </MenuTrigger>
      <MenuContent className='menu-content'>
        <MenuItem
          className='menu-item'
          value='edit'
          disabled={isPending}
          onClick={onEdit}
        >
          <Box as={Pencil} width='12px' height='12px' />
          <Text ml={2} fontSize='xs'>
            Edit
          </Text>
        </MenuItem>
        <MenuItem
          className='menu-item'
          value='duplicate'
          disabled={isPending}
          onClick={onDuplicate}
        >
          <Box as={Copy} width='12px' height='12px' />
          <Text ml={2} fontSize='xs'>
            Duplicate
          </Text>
        </MenuItem>
        <MenuItem
          className='menu-item'
          value='delete'
          disabled={isPending}
          onClick={onOpenDeleteDialog}
        >
          <Box as={Trash2} width='12px' height='12px' />
          <Text ml={2} fontSize='xs'>
            Delete
          </Text>
        </MenuItem>
        {protocol === 'tcp' && (
          <>
            <MenuItem
              className='menu-item'
              value='http-logs'
              disabled={isPending}
              onClick={onToggleHttpLogs}
            >
              <FileIcon size={12} />
              <Text ml={2} fontSize='xs'>
                {httpLogsEnabled === true ? 'Disable' : 'Enable'} HTTP Logs
              </Text>
            </MenuItem>
            {httpLogsEnabled === true && (
              <MenuItem
                className='menu-item'
                value='open-http-logs'
                disabled={isPending}
                onClick={onInspectLogs}
              >
                <FileIcon size={12} />
                <Text ml={2} fontSize='xs'>
                  Open HTTP Logs File
                </Text>
              </MenuItem>
            )}
            <MenuItem
              className='menu-item'
              value='http-logs-config'
              disabled={isPending}
              onClick={onOpenHttpLogsConfig}
            >
              <SettingsIcon size={12} />
              <Text ml={2} fontSize='xs'>
                HTTP Logs Settings
              </Text>
            </MenuItem>
          </>
        )}
      </MenuContent>
    </MenuRoot>
  )
}
