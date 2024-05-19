import React from 'react'
import { FaGithub } from 'react-icons/fa'
import { MdAdd, MdFileDownload, MdFileUpload, MdSettings } from 'react-icons/md'

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

import { FooterProps } from '../../types'

import BulkDeleteButton from './BulkDeleteButton'
import SyncConfigsButton from './SyncConfigsButton'

const Footer: React.FC<FooterProps> = ({
  openModal,
  openGitSyncModal,
  handleExportConfigs,
  handleImportConfigs,
  onConfigsSynced,
  credentialsSaved,
  setCredentialsSaved,
  isGitSyncModalOpen,
  setPollingInterval,
  pollingInterval,
  selectedConfigs,
  setSelectedConfigs,
  configs,
  setConfigs,
}) => {
  const borderColor = useColorModeValue('gray.500', 'gray.700')

  return (
    <Flex
      as='footer'
      direction='row'
      justifyContent='space-between'
      wrap='wrap'
      bg='gray.800'
      boxShadow='0px 0px 8px 3px rgba(0, 0, 0, 0.2)'
      borderRadius='lg'
      border='1px'
      borderColor={borderColor}
      width='100%'
      padding='0.3rem'
    >
      <Flex justifyContent='flex-start'>
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
          <MenuList zIndex='popover' fontSize='sm' minW='150px'>
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
        </Menu>
        <Tooltip
          label='Add New Config'
          placement='top'
          fontSize='sm'
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
        <BulkDeleteButton
          setSelectedConfigs={setSelectedConfigs}
          selectedConfigs={selectedConfigs}
          configs={configs}
          setConfigs={setConfigs}
        />
      </Flex>

      <Flex flexGrow={1} justifyContent='flex-end' alignItems='center'>
        <Tooltip
          label='Configure Git Sync'
          placement='top'
          fontSize='sm'
          lineHeight='tight'
        >
          <Button
            variant='outline'
            colorScheme='facebook'
            onClick={openGitSyncModal}
            size='sm'
            aria-label='Sync Configs'
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
          isGitSyncModalOpen={isGitSyncModalOpen}
          setPollingInterval={setPollingInterval}
          pollingInterval={pollingInterval}
        />
      </Flex>
    </Flex>
  )
}

export default Footer
