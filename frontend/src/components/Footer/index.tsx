import React, { useCallback, useState } from 'react'
import { Menu as MenuIcon } from 'lucide-react'
import { FaGithub } from 'react-icons/fa'
import {
  MdAdd,
  MdBuild,
  MdFileDownload,
  MdFileUpload,
  MdSettings,
} from 'react-icons/md'
import { RiInstallLine, RiUninstallLine } from 'react-icons/ri'

import { Box, Group } from '@chakra-ui/react'
import { invoke } from '@tauri-apps/api/core'

import AutoImportModal from '@/components/AutoImportModal'
import BulkDeleteButton from '@/components/Footer/BulkDeleteButton'
import SyncConfigsButton from '@/components/Footer/SyncConfigsButton'
import { Button } from '@/components/ui/button'
import {
  DialogBody,
  DialogCloseTrigger,
  DialogContent,
  DialogHeader,
  DialogRoot,
  DialogTitle,
} from '@/components/ui/dialog'
import {
  MenuContent,
  MenuItem,
  MenuRoot,
  MenuTrigger,
  MenuTriggerItem,
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
  syncStatus,
  onSyncComplete,
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

  const [helperActionResult, setHelperActionResult] = useState<{
    success: boolean
    message: string
    action: 'install' | 'uninstall'
  } | null>(null)

  const handleInstallHelper = async () => {
    try {
      const result = await invoke<boolean>('install_helper')

      if (result) {
        console.log('Helper successfully installed')
        setHelperActionResult({
          success: true,
          message: 'kftray-helper was successfully installed',
          action: 'install',
        })
      }
    } catch (error) {
      console.error('Failed to install helper:', error)
      setHelperActionResult({
        success: false,
        message: String(error),
        action: 'install',
      })
    }
  }

  const handleUninstallHelper = async () => {
    try {
      const result = await invoke<boolean>('remove_helper')

      if (result) {
        console.log('Helper successfully uninstalled')
        setHelperActionResult({
          success: true,
          message: 'kftray-helper was successfully uninstalled',
          action: 'uninstall',
        })
      }
    } catch (error) {
      console.error('Failed to uninstall helper:', error)
      setHelperActionResult({
        success: false,
        message: String(error),
        action: 'uninstall',
      })
    }
  }

  const closeHelperActionDialog = () => {
    setHelperActionResult(null)
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

      <MenuRoot>
        <MenuTriggerItem
          value='helper-submenu'
          startIcon={<Box as={MdBuild} width='12px' height='12px' />}
        >
          <Box fontSize='11px'>Helper</Box>
        </MenuTriggerItem>
        <MenuContent>
          <MenuItem value='install-helper' onClick={handleInstallHelper}>
            <Box as={RiInstallLine} width='12px' height='12px' />
            <Box fontSize='11px'>Install kftray-helper</Box>
          </MenuItem>

          <MenuItem value='uninstall-helper' onClick={handleUninstallHelper}>
            <Box as={RiUninstallLine} width='12px' height='12px' />
            <Box fontSize='11px'>Uninstall kftray-helper</Box>
          </MenuItem>
        </MenuContent>
      </MenuRoot>
    </>
  )

  return (
    <>
      <DialogRoot
        open={!!helperActionResult}
        onOpenChange={open => !open && closeHelperActionDialog()}
      >
        <DialogContent
          maxWidth='400px'
          width='350px'
          bg='#111111'
          borderRadius='lg'
          border='1px solid rgba(255, 255, 255, 0.08)'
          overflow='hidden'
        >
          <DialogHeader
            p={1.5}
            bg='#161616'
            borderBottom='1px solid rgba(255, 255, 255, 0.05)'
          >
            <DialogTitle fontSize='sm' fontWeight='medium' color='gray.100'>
              {helperActionResult?.success
                ? helperActionResult.action === 'install'
                  ? 'Installation Successful'
                  : 'Uninstallation Successful'
                : helperActionResult?.action === 'install'
                  ? 'Installation Failed'
                  : 'Uninstallation Failed'}
            </DialogTitle>
            <DialogCloseTrigger onClick={closeHelperActionDialog} />
          </DialogHeader>

          <DialogBody p={3}>
            <Box
              p={3}
              bg={
                helperActionResult?.success
                  ? 'rgba(56, 161, 105, 0.1)'
                  : 'rgba(229, 62, 62, 0.1)'
              }
              borderRadius='md'
              border={`1px solid ${helperActionResult?.success ? 'rgba(56, 161, 105, 0.2)' : 'rgba(229, 62, 62, 0.2)'}`}
            >
              {helperActionResult && (
                <Box
                  fontSize='xs'
                  color={helperActionResult.success ? 'green.300' : 'red.300'}
                >
                  {helperActionResult.message}
                </Box>
              )}
            </Box>

            <Box display='flex' justifyContent='flex-end' mt={4}>
              <Button
                onClick={closeHelperActionDialog}
                size='xs'
                bg={helperActionResult?.success ? 'green.600' : 'red.600'}
                _hover={{
                  bg: helperActionResult?.success ? 'green.700' : 'red.700',
                }}
                height='28px'
              >
                Close
              </Button>
            </Box>
          </DialogBody>
        </DialogContent>
      </DialogRoot>

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
            syncStatus={syncStatus}
            onSyncComplete={onSyncComplete}
          />
        </Group>

        <AutoImportModal
          isOpen={isAutoImportModalOpen}
          onClose={() => setIsAutoImportModalOpen(false)}
        />
      </Box>
    </>
  )
}

export default Footer
