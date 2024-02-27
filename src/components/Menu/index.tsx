import React from 'react'
import { IoSettingsOutline } from 'react-icons/io5'
import { MdAdd, MdFileDownload, MdFileUpload, MdSettings } from 'react-icons/md'

import {
  Box,
  IconButton,
  Menu,
  MenuButton,
  MenuItem,
  MenuList,
} from '@chakra-ui/react'

import { MenuProps } from '../../types'
import SyncConfigsButton from '../SyncConfigsButton'

const MenuOptions: React.FC<MenuProps> = ({
  openModal,
  openSettingsModal,
  handleExportConfigs,
  handleImportConfigs,
  onConfigsSynced,
  credentialsSaved,
  setCredentialsSaved,
  isSettingsModalOpen,
}) => {
  return (
    <Box display='flex' justifyContent='flex-start' width='100%' mt={5}>
      <Menu placement='top'>
        <MenuButton
          as={IconButton}
          aria-label='Options'
          icon={<IoSettingsOutline />}
          size='sm'
          colorScheme='facebook'
          variant='outline'
        />
        <MenuList zIndex='popover'>
          <MenuItem icon={<MdAdd />} onClick={openModal} isDisabled={credentialsSaved}>
            Add New Config
          </MenuItem>
          <MenuItem icon={<MdFileUpload />} onClick={handleExportConfigs}>
            Export Local File
          </MenuItem>
          <MenuItem
            icon={<MdFileDownload />}
            isDisabled={credentialsSaved}
            onClick={handleImportConfigs}
          >
            Import Local File
          </MenuItem>
          <MenuItem icon={<MdSettings />} onClick={openSettingsModal}>
            Configure Git Sync
          </MenuItem>
        </MenuList>
      </Menu>
      <Box display='flex' justifyContent='flex-end' width='100%' mt={2}>
        <SyncConfigsButton
          serviceName='kftray'
          accountName='github_config'
          onConfigsSynced={onConfigsSynced}
          onSyncFailure={error => console.error('Sync failed:', error)}
          credentialsSaved={credentialsSaved}
          setCredentialsSaved={setCredentialsSaved}
          isSettingsModalOpen={isSettingsModalOpen}
        />
      </Box>
    </Box>
  )
}

export default MenuOptions
