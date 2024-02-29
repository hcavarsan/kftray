import React from 'react'
import { IoSettingsOutline } from 'react-icons/io5'
import {
  MdAdd,
  MdDelete,
  MdFileDownload,
  MdFileUpload,
  MdSettings,
} from 'react-icons/md'

import {
  Box,
  Flex,
  IconButton,
  Menu,
  MenuButton,
  MenuItem,
  MenuList,
  Spacer,
  Tooltip,
  useColorModeValue,
} from '@chakra-ui/react'

import { FooterMenu } from '../../types'
import SyncConfigsButton from '../SyncConfigsButton'

const FooterMenu: React.FC<FooterMenu> = ({
  openModal,
  openSettingsModal,
  handleExportConfigs,
  handleImportConfigs,
  onConfigsSynced,
  credentialsSaved,
  setCredentialsSaved,
  isSettingsModalOpen,
  selectedConfigs,
  handleDeleteConfigs,
}) => {
  const borderColor = useColorModeValue('gray.500', 'gray.700')

  return (
    <Flex
      as='footer'
      direction='row'
      justifyContent='flex-start'
      top='0'
      bg='gray.800'
      py='1'
      px='1'
      boxShadow='lg'
      borderRadius='lg'
      border='1px'
      width='99%'
      borderColor={borderColor}
      mb='1'
    >
      <Menu placement='top-end'>
        <Tooltip
          label='Configurations'
          placement='top'
          fontSize='xs'
          lineHeight='tight'
        >
          <MenuButton
            as={IconButton}
            aria-label='Options'
            icon={<IoSettingsOutline />}
            size='sm'
            colorScheme='facebook'
            variant='outline'
            borderColor={borderColor}
          />
        </Tooltip>
        <MenuList zIndex='popover'>
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
        <Tooltip
          label='Add New Config'
          placement='top'
          fontSize='xs'
          lineHeight='tight'
        >
          <IconButton
            variant='outline'
            icon={<MdAdd />}
            colorScheme='facebook'
            onClick={openModal}
            isDisabled={credentialsSaved}
            size='sm'
            ml={2}
            aria-label='Add New Config'
            borderColor={borderColor}
          />
        </Tooltip>
        {selectedConfigs.length > 0 && (
          <Tooltip
            label='Delete Configs'
            placement='top'
            fontSize='xs'
            lineHeight='tight'
          >
            <IconButton
              colorScheme='red'
              variant='outline'
              onClick={() =>
                handleDeleteConfigs(selectedConfigs.map(config => config.id))
              }
              size='sm'
              aria-label='Delete selected configs'
              icon={<MdDelete />}
              ml={2}
            />
          </Tooltip>
        )}
        <Spacer />
        <SyncConfigsButton
          serviceName='kftray'
          accountName='github_config'
          onConfigsSynced={onConfigsSynced}
          onSyncFailure={error => console.error('Sync failed:', error)}
          credentialsSaved={credentialsSaved}
          setCredentialsSaved={setCredentialsSaved}
          isSettingsModalOpen={isSettingsModalOpen}
        />
      </Menu>
    </Flex>
  )
}

export default FooterMenu
