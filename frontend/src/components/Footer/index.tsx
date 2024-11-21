import React, { useCallback, useState } from 'react'
import { Menu as MenuIcon } from 'lucide-react'
import { FaGithub } from 'react-icons/fa'
import { MdAdd, MdFileDownload, MdFileUpload, MdSettings } from 'react-icons/md'

import { Box, Group } from '@chakra-ui/react'
import { invoke } from '@tauri-apps/api/tauri'

import AutoImportModal from '@/components/AutoImportModal'
import BulkDeleteButton from '@/components/Footer/BulkDeleteButton'
import SyncConfigsButton from '@/components/Footer/SyncConfigsButton'
import { Button } from '@/components/ui/button'
import {
  MenuContent,
  MenuItem,
  MenuRoot,
  MenuTrigger,
} from '@/components/ui/menu'
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

  const handleSyncFailure = useCallback((error: Error) => {
    console.error('Sync failed:', error)
  }, [])

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
      <MenuItem value='export' onClick={handleExportConfigs}>
        <Box as={MdFileUpload} width='12px' height='12px' />
        <Box fontSize='11px'>Export Local File</Box>
      </MenuItem>

      <MenuItem
        value='import'
        onClick={handleImportConfigs}
        disabled={credentialsSaved}
      >
        <Box as={MdFileDownload} width='12px' height='12px' />
        <Box fontSize='11px'>Import Local File</Box>
      </MenuItem>

      <MenuItem
        value='clear-logs'
        onClick={handleClearLogs}
        disabled={logState.size === 0 || logState.fetchError}
      >
        <Box as={MdSettings} width='12px' height='12px' />
        <Box fontSize='11px'>
          Prune Logs ({(logState.size / (1024 * 1024)).toFixed(2)} MB)
        </Box>
      </MenuItem>

      <MenuItem
        value='auto-import'
        onClick={() => setIsAutoImportModalOpen(true)}
      >
        <Box as={MdSettings} width='12px' height='12px' />
        <Box fontSize='11px'>Auto Import</Box>
      </MenuItem>
    </>
  )

  return (
    <Box
      display='flex'
      alignItems='center'
      justifyContent='space-between'
      width='100%'
      bg='#161616'
      px={3}
      py={2}
      borderRadius='lg'
      border='1px solid rgba(255, 255, 255, 0.08)'
      position='relative'
      mt='-1px'
      height='50px'
    >
      {/* Left Section */}
      <Group display='flex' alignItems='center' gap={2}>
        <MenuRoot>
          <MenuTrigger asChild>
            <Button
              size='sm'
              variant='ghost'
              onClick={fetchLogSize}
              height='32px'
              minWidth='32px'
              bg='whiteAlpha.50'
              px={1.5}
              borderRadius='md'
              border='1px solid rgba(255, 255, 255, 0.08)'
              _hover={{ bg: 'whiteAlpha.100' }}
            >
              <Box as={MenuIcon} width='12px' height='12px' />
            </Button>
          </MenuTrigger>
          <MenuContent>{renderMenuItems()}</MenuContent>
        </MenuRoot>

        <Tooltip
          content='Add New Config'
          portalled
          positioning={{
            strategy: 'absolute',
            placement: 'top-end',
            offset: { mainAxis: 8, crossAxis: 0 },
          }}
        >
          <Button
            size='sm'
            variant='ghost'
            onClick={openModal}
            disabled={credentialsSaved}
            height='32px'
            minWidth='32px'
            bg='whiteAlpha.50'
            px={1.5}
            borderRadius='md'
            border='1px solid rgba(255, 255, 255, 0.08)'
            _hover={{ bg: 'whiteAlpha.100' }}
          >
            <Box as={MdAdd} width='12px' height='12px' />
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
        <Tooltip
          content='Configure Git Sync'
          portalled
          positioning={{
            strategy: 'absolute',
            placement: 'top-end',
            offset: { mainAxis: 8, crossAxis: 0 },
          }}
        >
          <Button
            size='sm'
            variant='ghost'
            onClick={openGitSyncModal}
            height='32px'
            minWidth='32px'
            bg='whiteAlpha.50'
            px={1.5}
            borderRadius='md'
            border='1px solid rgba(255, 255, 255, 0.08)'
            _hover={{ bg: 'whiteAlpha.100' }}
          >
            <Box display='flex' alignItems='center' gap={1}>
              <Box as={FaGithub} width='14px' height='14px' />
              <Box as={MdSettings} width='14px' height='14px' />
            </Box>
          </Button>
        </Tooltip>

        <SyncConfigsButton
          serviceName='kftray'
          accountName='github_config'
          onSyncFailure={handleSyncFailure}
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
