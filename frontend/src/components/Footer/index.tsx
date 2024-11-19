import React, { useState } from 'react'
import { Menu as MenuIcon } from 'lucide-react'
import { FaGithub } from 'react-icons/fa'
import { MdAdd, MdFileDownload, MdFileUpload, MdSettings } from 'react-icons/md'

import { Box, Group } from '@chakra-ui/react'
import { invoke } from '@tauri-apps/api/tauri'

import AutoImportModal from '@/components/AutoImportModal'
import BulkDeleteButton from '@/components/Footer/BulkDeleteButton'
import SyncConfigsButton from '@/components/Footer/SyncConfigsButton'
import { Button } from '@/components/ui/button'
import { MenuContent, MenuItem, MenuRoot, MenuTrigger } from '@/components/ui/menu'
import { Tooltip } from '@/components/ui/tooltip'
import { FooterProps } from '@/types'

const Footer: React.FC<FooterProps> = ({
  openModal,
  openGitSyncModal,
  handleExportConfigs,
  handleImportConfigs,
  credentialsSaved,
  setCredentialsSaved,
  isGitSyncModalOpen,
  setPollingInterval,
  pollingInterval,
  selectedConfigs,
  setSelectedConfigs,
  configs,
}) => {
  const [logState, setLogState] = useState({
    size: 0,
    fetchError: false,
  })
  const [isAutoImportModalOpen, setIsAutoImportModalOpen] = useState(false)

  const fetchLogSize = async () => {
    try {
      const size = await invoke<number>('get_http_log_size')


      setLogState({ size, fetchError: false })
    } catch (error) {
      console.error('Failed to fetch log size:', error)
      setLogState(prev => ({ ...prev, fetchError: true }))
    }
  }

  const handleClearLogs = async () => {
    try {
      await invoke('clear_http_logs')
      setLogState(prev => ({ ...prev, size: 0 }))
    } catch (error) {
      console.error('Failed to clear logs:', error)
    }
  }

  const renderMenuItems = () => (
    <>
      <MenuItem
        value='export'
        onClick={handleExportConfigs}
        className="footer-menu-item"
      >
        <Box as={MdFileUpload} width='12px' height='12px' />
        <Box>Export Local File</Box>
      </MenuItem>

      <MenuItem
        value='import'
        onClick={handleImportConfigs}
        disabled={credentialsSaved}
        className="footer-menu-item"
      >
        <Box as={MdFileDownload} width='12px' height='12px' />
        <Box>Import Local File</Box>
      </MenuItem>

      <MenuItem
        value='clear-logs'
        onClick={handleClearLogs}
        disabled={logState.size === 0 || logState.fetchError}
        className="footer-menu-item"
      >
        <Box as={MdSettings} width='12px' height='12px' />
        <Box>Prune Logs ({(logState.size / (1024 * 1024)).toFixed(2)} MB)</Box>
      </MenuItem>

      <MenuItem
        value='auto-import'
        onClick={() => setIsAutoImportModalOpen(true)}
        className="footer-menu-item"
      >
        <Box as={MdSettings} width='12px' height='12px' />
        <Box>Auto Import</Box>
      </MenuItem>
    </>
  )

  return (
    <Box className="footer-container">
      {/* Left Section */}
      <Group display='flex' alignItems='center' gap={2}>
        <MenuRoot>
          <MenuTrigger asChild>
            <Button
              size='sm'
              variant='ghost'
              onClick={fetchLogSize}
              className="menu-trigger-button"
            >
              <Box as={MenuIcon} width='16px' height='16px' />
            </Button>
          </MenuTrigger>
          <MenuContent className="menu-content">
            {renderMenuItems()}
          </MenuContent>
        </MenuRoot>

        <Tooltip content='Add New Config'>
          <Button
            size='sm'
            variant='ghost'
            onClick={openModal}
            disabled={credentialsSaved}
            className="add-config-button"
          >
            <Box as={MdAdd} width='16px' height='16px' />
          </Button>
        </Tooltip>

        <BulkDeleteButton
          setSelectedConfigs={setSelectedConfigs}
          selectedConfigs={selectedConfigs}
          configs={configs}
        />
      </Group>

      {/* Right Section */}
      <Group display='flex' alignItems='center' gap={2}>
        <Tooltip content='Configure Git Sync'>
          <Button
            size='sm'
            variant='ghost'
            onClick={openGitSyncModal}
            className="git-sync-button"
          >
            <Box display='flex' alignItems='center' gap={1}>
              <Box as={FaGithub} width='16px' height='16px' />
              <Box as={MdSettings} width='16px' height='16px' />
            </Box>
          </Button>
        </Tooltip>

        <SyncConfigsButton
          serviceName='kftray'
          accountName='github_config'
          onSyncFailure={error => console.error('Sync failed:', error)}
          credentialsSaved={credentialsSaved}
          setCredentialsSaved={setCredentialsSaved}
          isGitSyncModalOpen={isGitSyncModalOpen}
          setPollingInterval={setPollingInterval}
          pollingInterval={pollingInterval}
        />
      </Group>

      <AutoImportModal
        isOpen={isAutoImportModalOpen}
        onClose={() => setIsAutoImportModalOpen(false)}
      />
    </Box>
  )
}

export default Footer
