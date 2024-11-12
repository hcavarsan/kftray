import React, { useState } from 'react'
import { Menu as MenuIcon } from 'lucide-react'
import { FaGithub } from 'react-icons/fa'
import { MdAdd, MdFileDownload, MdFileUpload, MdSettings } from 'react-icons/md'

import { Box } from '@chakra-ui/react'
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
  const [logSize, setLogSize] = useState<number>(0)
  const [fetchError, setFetchError] = useState<boolean>(false)
  const [isAutoImportModalOpen, setIsAutoImportModalOpen] = useState(false)

  const fetchLogSize = async () => {
    try {
      const size = await invoke<number>('get_http_log_size')

      setLogSize(size)
      setFetchError(false)
    } catch (error) {
      console.error('Failed to fetch log size:', error)
      setFetchError(true)
    }
  }

  const handleClearLogs = async () => {
    try {
      await invoke('clear_http_logs')
      setLogSize(0)
    } catch (error) {
      console.error('Failed to clear logs:', error)
    }
  }

  return (
    <Box
      display="flex"
      alignItems="center"
      justifyContent="space-between"
      width="100%"
      bg="#161616"
      px={3}
      py={2}
      borderRadius="md"
      border="1px solid rgba(255, 255, 255, 0.08)"
    >
      {/* Left Section */}
      <Box display="flex" alignItems="center" gap={2}>
        {/* Menu Button */}
        <MenuRoot>
          <MenuTrigger asChild>
            <Button
              variant="ghost"
              size="xs"
              onClick={fetchLogSize}
              height="24px"
              width="24px"
              minWidth="24px"
              p={0}
              bg="whiteAlpha.50"
              _hover={{ bg: 'whiteAlpha.100' }}
            >
              <Box as={MenuIcon} width={4} height={4} />
            </Button>
          </MenuTrigger>
          <MenuContent
            css={{
              backgroundColor: '#1A1A1A',
              border: '1px solid rgba(255, 255, 255, 0.08)',
              padding: '4px',
              gap: '2px',
            }}
          >
            <MenuItem
              value="export"
              onClick={handleExportConfigs}
              css={{
                fontSize: '11px',
                padding: '6px 8px',
                borderRadius: '4px',
                gap: '8px',
                '&:hover': {
                  backgroundColor: 'rgba(255, 255, 255, 0.05)',
                },
              }}
            >
              <Box as={MdFileUpload} width={5} height={5} />
              <Box>Export Local File</Box>
            </MenuItem>

            <MenuItem
              value="import"
              onClick={handleImportConfigs}
              disabled={credentialsSaved}
              css={{
                fontSize: '11px',
                padding: '6px 8px',
                borderRadius: '4px',
                gap: '8px',
                '&:hover': {
                  backgroundColor: 'rgba(255, 255, 255, 0.05)',
                },
                '&[disabled]': {
                  opacity: 0.5,
                  pointerEvents: 'none',
                },
              }}
            >
              <Box as={MdFileDownload} width={5} height={5} />
              <Box>Import Local File</Box>
            </MenuItem>

            <MenuItem
              value="clear-logs"
              onClick={handleClearLogs}
              disabled={logSize === 0 || fetchError}
              css={{
                fontSize: '11px',
                padding: '6px 8px',
                borderRadius: '4px',
                gap: '8px',
                '&:hover': {
                  backgroundColor: 'rgba(255, 255, 255, 0.05)',
                },
                '&[disabled]': {
                  opacity: 0.5,
                  pointerEvents: 'none',
                },
              }}
            >
              <Box as={MdSettings} width={5} height={5} />
              <Box>Prune Logs ({(logSize / (1024 * 1024)).toFixed(2)} MB)</Box>
            </MenuItem>

            <MenuItem
              value="auto-import"
              onClick={() => setIsAutoImportModalOpen(true)}
              css={{
                fontSize: '11px',
                padding: '6px 8px',
                borderRadius: '4px',
                gap: '8px',
                '&:hover': {
                  backgroundColor: 'rgba(255, 255, 255, 0.05)',
                },
              }}
            >
              <Box as={MdSettings} width={6} height={6} />
              <Box>Auto Import</Box>
            </MenuItem>
          </MenuContent>
        </MenuRoot>

        {/* Add Button */}
        <Tooltip content="Add New Config">
          <Button
            variant="ghost"
            size="sm"
            onClick={openModal}
            disabled={credentialsSaved}
            height="25px"
            width="25px"
            minWidth="25px"
            p={0}
            bg="whiteAlpha.50"
            _hover={{ bg: 'whiteAlpha.100' }}
          >
            <Box as={MdAdd} width={4} height={4} />
          </Button>
        </Tooltip>

        <BulkDeleteButton
          setSelectedConfigs={setSelectedConfigs}
          selectedConfigs={selectedConfigs}
          configs={configs}
        />
      </Box>

      {/* Right Section */}
      <Box display="flex" alignItems="center" gap={2}>
        <Tooltip content="Configure Git Sync">
          <Button
            variant="ghost"
            size="2xs"
            onClick={openGitSyncModal}
            height="24px"
            minWidth="40px"
            px={2}
            bg="whiteAlpha.50"
            _hover={{ bg: 'whiteAlpha.100' }}
          >
            <Box display="flex" alignItems="center" gap={1}>
              <Box as={FaGithub} width={4} height={4} />
              <Box as={MdSettings} width={4} height={4} />
            </Box>
          </Button>
        </Tooltip>

        <SyncConfigsButton
          serviceName="kftray"
          accountName="github_config"
          onSyncFailure={error => console.error('Sync failed:', error)}
          credentialsSaved={credentialsSaved}
          setCredentialsSaved={setCredentialsSaved}
          isGitSyncModalOpen={isGitSyncModalOpen}
          setPollingInterval={setPollingInterval}
          pollingInterval={pollingInterval}
        />
      </Box>

      <AutoImportModal
        isOpen={isAutoImportModalOpen}
        onClose={() => setIsAutoImportModalOpen(false)}
      />

    </Box>
  )
}

export default Footer
