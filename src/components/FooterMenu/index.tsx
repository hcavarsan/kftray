import React from 'react'
import { FaGithub } from 'react-icons/fa'
import {
  MdAdd,
  MdDelete,
  MdFileDownload,
  MdFileUpload,
  MdSettings,
} from 'react-icons/md'

import { HamburgerIcon } from '@chakra-ui/icons'
import {
  Box,
  Button,
  Flex,
  HStack,
  IconButton,
  Menu,
  MenuButton,
  MenuItem,
  MenuList,
  Tooltip,
  useColorModeValue,
} from '@chakra-ui/react'

import { FooterMenuProps } from '../../types'
import SyncConfigsButton from '../SyncConfigsButton'

const FooterMenu: React.FC<FooterMenuProps> = ({
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
  setPollingInterval,
  pollingInterval,
}) => {
  const borderColor = useColorModeValue('gray.500', 'gray.700')

  return (
    <Flex
      as='footer'
      direction={{ base: 'column', sm: 'row' }}
      justifyContent='space-between'
      wrap='wrap'
      bg='gray.800'
      py={{ base: 2, sm: 1 }}
      px={{ base: 2, sm: 1 }}
      boxShadow='0px 0px 8px 3px rgba(0, 0, 0, 0.2)'
      borderRadius='lg'
      border='1px'
      borderColor={borderColor}
      mb={3}
      width='97%'
    >
      <Flex align='center' mb={{ base: 2, sm: 0 }}>
        <Menu placement='top-end'>
          <MenuButton
            as={IconButton}
            aria-label='Options'
            icon={<HamburgerIcon />}
            size='sm'
            colorScheme='facebook'
            variant='outline'
            borderColor={borderColor}
          />
          <MenuList zIndex='popover' fontSize='xs' minW='150px'>
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
                borderColor={borderColor}
                icon={<MdDelete />}
                ml={2}
              />
            </Tooltip>
          )}
        </Menu>
      </Flex>

      <Flex align='center' flexGrow={1} justifyContent={{ sm: 'flex-end' }}>
        <Tooltip
          label='Configure Git Sync'
          placement='top'
          fontSize='xs'
          lineHeight='tight'
        >
          <Button
            variant='outline'
            colorScheme='facebook'
            onClick={openSettingsModal}
            size='sm'
            aria-label='Sync Configs'
            justifyContent='center'
            borderColor='gray.700'
            mr={2}
          >
            <HStack spacing={1}>
              <Box as={FaGithub} />
              <MdSettings />
            </HStack>
          </Button>
        </Tooltip>
        <SyncConfigsButton
          serviceName='kftray'
          accountName='github_config'
          onConfigsSynced={onConfigsSynced}
          onSyncFailure={error => console.error('Sync failed:', error)}
          credentialsSaved={credentialsSaved}
          setCredentialsSaved={setCredentialsSaved}
          isSettingsModalOpen={isSettingsModalOpen}
          setPollingInterval={setPollingInterval}
          pollingInterval={pollingInterval}
        />
      </Flex>
    </Flex>
  )
}

export default FooterMenu
