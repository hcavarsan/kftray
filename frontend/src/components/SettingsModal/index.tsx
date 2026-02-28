import React, { useEffect, useState } from 'react'
import { Download, FileText, RefreshCw, Shield, Trash2 } from 'lucide-react'

import { Box, Dialog, Flex, Input, Stack, Text } from '@chakra-ui/react'
import { app } from '@tauri-apps/api'
import { invoke } from '@tauri-apps/api/core'

import type { LogFileInfo, LogSettings } from '@/components/LogViewer'
import McpServerSettings from '@/components/SettingsModal/McpServerSettings'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { DialogCloseTrigger } from '@/components/ui/dialog'
import { toaster } from '@/components/ui/toaster'

interface SettingsModalProps {
  isOpen: boolean
  onClose: () => void
}

const SettingsModal: React.FC<SettingsModalProps> = ({ isOpen, onClose }) => {
  const [disconnectTimeout, setDisconnectTimeout] = useState<string>('0')
  const [networkMonitor, setNetworkMonitor] = useState<boolean>(true)
  const [networkMonitorStatus, setNetworkMonitorStatus] =
    useState<boolean>(false)
  const [autoUpdateEnabled, setAutoUpdateEnabled] = useState<boolean>(true)
  const [lastUpdateCheckDisplay, setLastUpdateCheckDisplay] = useState<string>('Never')
  const [currentVersion, setCurrentVersion] = useState<string>('')
  const [latestVersion, setLatestVersion] = useState<string>('')
  const [updateStatus, setUpdateStatus] = useState<
    'idle' | 'checking' | 'available' | 'up-to-date' | 'error'
  >('idle')
  const [isLoading, setIsLoading] = useState(false)
  const [isSaving, setIsSaving] = useState(false)
  const [isCheckingUpdates, setIsCheckingUpdates] = useState(false)
  const [isUpdating, setIsUpdating] = useState(false)

  const [sslEnabled, setSslEnabled] = useState<boolean>(false)
  const [sslCertValidityDays, setSslCertValidityDays] = useState<string>('365')

  const [logRetentionCount, setLogRetentionCount] = useState<string>('10')
  const [logRetentionDays, setLogRetentionDays] = useState<string>('7')
  const [logFileCount, setLogFileCount] = useState<number>(0)
  const [logTotalSize, setLogTotalSize] = useState<number>(0)
  const [isCleaningLogs, setIsCleaningLogs] = useState(false)

  useEffect(() => {
    if (isOpen) {
      loadSettings()
      loadVersionInfo()
      loadLogInfo()
    }
  }, [isOpen])

  const loadSettings = async () => {
    try {
      setIsLoading(true)
      const settings = await invoke<Record<string, string>>('get_settings')

      setDisconnectTimeout(settings.disconnect_timeout_minutes || '0')
      setNetworkMonitor(settings.network_monitor === 'true')
      setNetworkMonitorStatus(settings.network_monitor_status === 'true')
      setAutoUpdateEnabled(settings.auto_update_enabled === 'true')

      const lastCheck = parseInt(settings.last_update_check || '0', 10)

      if (lastCheck > 0) {
        const date = new Date(lastCheck * 1000)

        setLastUpdateCheckDisplay(
          date.toLocaleDateString() + ' at ' + date.toLocaleTimeString(),
        )
      } else {
        setLastUpdateCheckDisplay('Never')
      }

      try {
        const sslSettings = await invoke<{
          ssl_enabled: boolean
          ssl_cert_validity_days: number
        }>('get_ssl_settings')

        setSslEnabled(sslSettings.ssl_enabled || false)
        setSslCertValidityDays(
          String(sslSettings.ssl_cert_validity_days || 365),
        )
      } catch (sslError) {
        console.error('Error loading SSL settings:', sslError)
        setSslEnabled(false)
        setSslCertValidityDays('365')
      }

      try {
        const logSettings = await invoke<LogSettings>('get_log_settings')

        setLogRetentionCount(String(logSettings.retention_count))
        setLogRetentionDays(String(logSettings.retention_days))
      } catch (logError) {
        console.error('Error loading log settings:', logError)
        setLogRetentionCount('10')
        setLogRetentionDays('7')
      }
    } catch (error) {
      console.error('Error loading settings:', error)
      toaster.error({
        title: 'Error',
        description: 'Failed to load settings',
        duration: 3000,
      })
    } finally {
      setIsLoading(false)
    }
  }

  const loadVersionInfo = async () => {
    try {
      const version = await app.getVersion()

      setCurrentVersion(version)
      setLatestVersion(version)
      setUpdateStatus('idle')
    } catch (error) {
      console.error('Error loading version info:', error)
      setUpdateStatus('error')
    }
  }

  const loadLogInfo = async () => {
    try {
      const files = await invoke<LogFileInfo[]>('list_log_files')

      setLogFileCount(files.length)
      setLogTotalSize(files.reduce((acc, f) => acc + f.size, 0))
    } catch (error) {
      console.error('Error loading log info:', error)
    }
  }

  const checkForUpdates = async () => {
    try {
      setIsCheckingUpdates(true)
      setUpdateStatus('checking')

      const versionInfo =
        await invoke<Record<string, string>>('get_version_info')

      setLatestVersion(versionInfo.latest_version || currentVersion)

      const isUpdateAvailable = versionInfo.update_available === 'true'

      if (versionInfo.update_available === 'error') {
        setUpdateStatus('error')
        toaster.error({
          title: 'Update Check Failed',
          description: 'Failed to check for updates. Please try again later.',
          duration: 4000,
        })
      } else if (isUpdateAvailable) {
        setUpdateStatus('available')
        toaster.success({
          title: 'Update Available',
          description: `Version ${versionInfo.latest_version} is now available!`,
          duration: 4000,
        })
      } else {
        setUpdateStatus('up-to-date')
        toaster.success({
          title: 'Up to Date',
          description: 'You are running the latest version.',
          duration: 3000,
        })
      }

      await loadSettings()
    } catch (error) {
      console.error('Error checking for updates:', error)
      setUpdateStatus('error')
      toaster.error({
        title: 'Update Check Failed',
        description: 'Failed to check for updates. Please try again later.',
        duration: 4000,
      })
    } finally {
      setIsCheckingUpdates(false)
      await loadSettings()
    }
  }

  const installUpdate = async () => {
    try {
      setIsUpdating(true)

      toaster.create({
        title: 'Installing Update',
        description:
          'The update is being downloaded and installed. App will restart automatically.',
        duration: 5000,
      })

      await invoke('install_update_silent')
    } catch (error) {
      console.error('Error installing update:', error)
      toaster.error({
        title: 'Update Failed',
        description: 'Failed to install the update. Please try again later.',
        duration: 4000,
      })
      setIsUpdating(false)
    }
  }

  const cleanupLogs = async () => {
    try {
      setIsCleaningLogs(true)
      const deleted = await invoke<number>('cleanup_old_logs')

      await loadLogInfo()

      if (deleted > 0) {
        toaster.success({
          title: 'Logs Cleaned',
          description: `Deleted ${deleted} old log file${deleted === 1 ? '' : 's'}`,
          duration: 3000,
        })
      } else {
        toaster.create({
          title: 'No Cleanup Needed',
          description: 'No log files met the cleanup criteria',
          duration: 3000,
        })
      }
    } catch (error) {
      console.error('Error cleaning logs:', error)
      toaster.error({
        title: 'Cleanup Failed',
        description: String(error),
        duration: 4000,
      })
    } finally {
      setIsCleaningLogs(false)
    }
  }

  const openLogsWindow = async () => {
    try {
      await invoke('open_log_viewer_window_cmd')
    } catch (error) {
      console.error('Error opening logs:', error)
      toaster.error({
        title: 'Error',
        description: 'Failed to open log viewer',
        duration: 3000,
      })
    }
  }

  const saveSettings = async () => {
    try {
      setIsSaving(true)
      const timeoutValue = parseInt(disconnectTimeout, 10)
      const certValidityValue = parseInt(sslCertValidityDays, 10)
      const logCountValue = parseInt(logRetentionCount, 10)
      const logDaysValue = parseInt(logRetentionDays, 10)

      if (isNaN(timeoutValue) || timeoutValue < 0) {
        toaster.error({
          title: 'Invalid Input',
          description: 'Please enter a valid number (0 or greater) for timeout',
          duration: 3000,
        })

        return
      }

      if (
        isNaN(certValidityValue) ||
        certValidityValue < 1 ||
        certValidityValue > 3650
      ) {
        toaster.error({
          title: 'Invalid Input',
          description: 'Certificate validity must be between 1 and 3650 days',
          duration: 3000,
        })

        return
      }

      if (isNaN(logCountValue) || logCountValue < 1 || logCountValue > 100) {
        toaster.error({
          title: 'Invalid Input',
          description: 'Log retention count must be between 1 and 100',
          duration: 3000,
        })

        return
      }

      if (isNaN(logDaysValue) || logDaysValue < 1 || logDaysValue > 365) {
        toaster.error({
          title: 'Invalid Input',
          description: 'Log retention days must be between 1 and 365',
          duration: 3000,
        })

        return
      }

      await invoke('update_disconnect_timeout', { minutes: timeoutValue })
      await invoke('update_network_monitor', { enabled: networkMonitor })
      await invoke('update_auto_update_enabled', { enabled: autoUpdateEnabled })

      const currentSslSettings = await invoke<{
        ssl_enabled: boolean
      }>('get_ssl_settings')
      const wasDisabled = !currentSslSettings.ssl_enabled
      const willBeEnabled = sslEnabled

      try {
        await invoke('set_ssl_settings', {
          sslEnabled,
          sslCertValidityDays: certValidityValue,
          sslAutoRegenerate: true,
          sslCaAutoInstall: true,
        })

        if (wasDisabled && willBeEnabled) {
          toaster.success({
            title: 'SSL/HTTPS Enabled Successfully',
            description:
              'SSL certificates have been generated and installed. You may need to restart your browser for SSL connections to work properly.',
            duration: 8000,
          })
        }
      } catch (sslError) {
        console.error('Error saving SSL settings:', sslError)
        toaster.error({
          title: 'SSL Settings Error',
          description:
            'Failed to save SSL settings, but other settings were saved',
          duration: 4000,
        })
      }

      try {
        await invoke('set_log_settings', {
          settings: {
            retention_count: logCountValue,
            retention_days: logDaysValue,
          },
        })
      } catch (logError) {
        console.error('Error saving log settings:', logError)
        toaster.error({
          title: 'Log Settings Error',
          description:
            'Failed to save log settings, but other settings were saved',
          duration: 4000,
        })
      }

      await loadSettings()

      if (!(wasDisabled && willBeEnabled)) {
        toaster.success({
          title: 'Settings Saved',
          description: 'All settings have been saved successfully',
          duration: 3000,
        })
      }

      onClose()
    } catch (error) {
      console.error('Error saving settings:', error)
      toaster.error({
        title: 'Error',
        description: 'Failed to save settings',
        duration: 3000,
      })
    } finally {
      setIsSaving(false)
    }
  }

  const handleTimeoutChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const value = e.target.value

    if (value === '' || /^\d+$/.test(value)) {
      setDisconnectTimeout(value)
    }
  }

  const handleCertValidityChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const value = e.target.value

    if (value === '' || (/^\d+$/.test(value) && parseInt(value, 10) <= 3650)) {
      setSslCertValidityDays(value)
    }
  }

  const handleLogRetentionCountChange = (
    e: React.ChangeEvent<HTMLInputElement>,
  ) => {
    const value = e.target.value

    if (value === '' || (/^\d+$/.test(value) && parseInt(value, 10) <= 100)) {
      setLogRetentionCount(value)
    }
  }

  const handleLogRetentionDaysChange = (
    e: React.ChangeEvent<HTMLInputElement>,
  ) => {
    const value = e.target.value

    if (value === '' || (/^\d+$/.test(value) && parseInt(value, 10) <= 365)) {
      setLogRetentionDays(value)
    }
  }

  const formatFileSize = (bytes: number): string => {
    if (bytes < 1024) {
      return `${bytes} B`
    }
    if (bytes < 1024 * 1024) {
      return `${(bytes / 1024).toFixed(1)} KB`
    }

    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
  }

  return (
    <Dialog.Root
      open={isOpen}
      onOpenChange={({ open }) => !open && onClose()}
      modal={true}
    >
      <Dialog.Backdrop
        bg='transparent'
        backdropFilter='blur(4px)'
        height='100vh'
      />
      <Dialog.Positioner overflow='hidden'>
        <Dialog.Content
          onClick={e => e.stopPropagation()}
          maxWidth='600px'
          width='90vw'
          height='92vh'
          bg='#111111'
          border='1px solid rgba(255, 255, 255, 0.08)'
          borderRadius='lg'
          overflow='hidden'
          position='absolute'
          my={2}
        >
          <DialogCloseTrigger
            style={{
              marginTop: '-4px',
            }}
          />

          <Dialog.Header
            p={3}
            bg='#161616'
            borderBottom='1px solid rgba(255, 255, 255, 0.05)'
          >
            <Text fontSize='sm' fontWeight='medium' color='gray.100'>
              Settings
            </Text>
          </Dialog.Header>

          <Dialog.Body
            p={3}
            overflowY='auto'
            css={{
              '&::-webkit-scrollbar': {
                width: '6px',
              },
              '&::-webkit-scrollbar-track': {
                background: 'transparent',
              },
              '&::-webkit-scrollbar-thumb': {
                background: 'rgba(255, 255, 255, 0.2)',
                borderRadius: '3px',
              },
              '&::-webkit-scrollbar-thumb:hover': {
                background: 'rgba(255, 255, 255, 0.3)',
              },
            }}
          >
            <Stack gap={2.5}>
              {/* Two Column Grid Layout */}
              <Box display='grid' gridTemplateColumns='1fr 1fr' gap={2.5}>
                {/* Left Column - Auto-disconnect Timeout */}
                <Box
                  bg='#161616'
                  p={2}
                  borderRadius='md'
                  border='1px solid rgba(255, 255, 255, 0.08)'
                  display='flex'
                  flexDirection='column'
                  height='100%'
                >
                  <Text fontSize='sm' fontWeight='500' color='white' mb={1}>
                    Auto-disconnect Timeout
                  </Text>
                  <Text
                    fontSize='xs'
                    color='whiteAlpha.600'
                    lineHeight='1.3'
                    flex='1'
                  >
                    Disconnect port forwards after specified time (min). Set to
                    0 to disable.
                  </Text>
                  <Box
                    borderTop='1px solid rgba(255, 255, 255, 0.06)'
                    mt={3}
                    pt={3}
                  >
                    <Flex align='center' justify='flex-end' gap={2}>
                      <Text fontSize='xs' color='whiteAlpha.500'>
                        Minutes:
                      </Text>
                      <Input
                        value={disconnectTimeout}
                        onChange={handleTimeoutChange}
                        placeholder='0'
                        size='xs'
                        width='45px'
                        height='22px'
                        bg='#111111'
                        border='1px solid rgba(255, 255, 255, 0.08)'
                        _hover={{ borderColor: 'rgba(255, 255, 255, 0.15)' }}
                        _focus={{ borderColor: 'blue.400', boxShadow: 'none' }}
                        color='white'
                        _placeholder={{ color: 'whiteAlpha.500' }}
                        disabled={isLoading}
                        textAlign='center'
                        fontSize='xs'
                      />
                    </Flex>
                  </Box>
                </Box>

                {/* Right Column - Network Monitor */}
                <Box
                  bg='#161616'
                  p={2}
                  borderRadius='md'
                  border='1px solid rgba(255, 255, 255, 0.08)'
                  display='flex'
                  flexDirection='column'
                  height='100%'
                >
                  <Flex align='center' gap={1.5} mb={1}>
                    <Text fontSize='sm' fontWeight='500' color='white'>
                      Network Monitor
                    </Text>
                    <Box
                      width='5px'
                      height='5px'
                      borderRadius='full'
                      bg={networkMonitorStatus ? 'green.400' : 'gray.500'}
                      title={networkMonitorStatus ? 'Running' : 'Stopped'}
                    />
                  </Flex>
                  <Text
                    fontSize='xs'
                    color='whiteAlpha.600'
                    lineHeight='1.3'
                    flex='1'
                  >
                    Monitor connectivity and reconnect port forwards when
                    network is restored.
                  </Text>
                  <Box
                    borderTop='1px solid rgba(255, 255, 255, 0.06)'
                    mt={3}
                    pt={3}
                  >
                    <Flex align='center' justify='flex-end' gap={2}>
                      <Text fontSize='xs' color='whiteAlpha.500'>
                        Enabled:
                      </Text>
                      <Checkbox
                        checked={networkMonitor}
                        onCheckedChange={e =>
                          setNetworkMonitor(e.checked === true)
                        }
                        disabled={isLoading}
                        size='sm'
                      />
                    </Flex>
                  </Box>
                </Box>

                {/* Left Column - SSL Configuration */}
                <Box
                  bg='#161616'
                  p={2}
                  borderRadius='md'
                  border='1px solid rgba(255, 255, 255, 0.08)'
                  display='flex'
                  flexDirection='column'
                  height='100%'
                >
                  <Flex align='center' gap={1.5} mb={1}>
                    <Box
                      as={Shield}
                      width='10px'
                      height='10px'
                      color='blue.400'
                    />
                    <Text fontSize='sm' fontWeight='500' color='white'>
                      SSL/HTTPS
                    </Text>
                    <Box
                      width='5px'
                      height='5px'
                      borderRadius='full'
                      bg={sslEnabled ? 'green.400' : 'gray.500'}
                      title={sslEnabled ? 'Enabled' : 'Disabled'}
                    />
                  </Flex>
                  <Text
                    fontSize='xs'
                    color='whiteAlpha.600'
                    lineHeight='1.3'
                    flex='1'
                  >
                    Enable HTTPS for port forwards with domain aliases. Creates
                    SSL certificates automatically.
                  </Text>
                  <Box
                    borderTop='1px solid rgba(255, 255, 255, 0.06)'
                    mt={3}
                    pt={3}
                  >
                    <Flex align='center' justify='flex-end' gap={2}>
                      <Text fontSize='xs' color='whiteAlpha.500'>
                        Enabled:
                      </Text>
                      <Checkbox
                        checked={sslEnabled}
                        onCheckedChange={e => setSslEnabled(e.checked === true)}
                        disabled={isLoading}
                        size='sm'
                      />
                    </Flex>
                  </Box>
                </Box>

                {/* Right Column - SSL Certificate Settings */}
                <Box
                  bg='#161616'
                  p={2}
                  borderRadius='md'
                  border='1px solid rgba(255, 255, 255, 0.08)'
                  display='flex'
                  flexDirection='column'
                  height='100%'
                  opacity={sslEnabled ? 1 : 0.5}
                >
                  <Text fontSize='sm' fontWeight='500' color='white' mb={1}>
                    Certificate Validity
                  </Text>
                  <Text
                    fontSize='xs'
                    color='whiteAlpha.600'
                    lineHeight='1.3'
                    flex='1'
                  >
                    Configure SSL certificate validity period. Certificates will
                    auto-regenerate and CA will be auto-installed.
                  </Text>
                  <Box
                    borderTop='1px solid rgba(255, 255, 255, 0.06)'
                    mt={3}
                    pt={3}
                  >
                    <Flex align='center' justify='flex-end' gap={2}>
                      <Text fontSize='xs' color='whiteAlpha.500'>
                        Validity (days):
                      </Text>
                      <Input
                        value={sslCertValidityDays}
                        onChange={handleCertValidityChange}
                        placeholder='365'
                        size='xs'
                        width='55px'
                        height='22px'
                        bg='#111111'
                        border='1px solid rgba(255, 255, 255, 0.08)'
                        _hover={{ borderColor: 'rgba(255, 255, 255, 0.15)' }}
                        _focus={{ borderColor: 'blue.400', boxShadow: 'none' }}
                        color='white'
                        _placeholder={{ color: 'whiteAlpha.500' }}
                        disabled={isLoading || !sslEnabled}
                        textAlign='center'
                        fontSize='xs'
                      />
                    </Flex>
                  </Box>
                </Box>

                {/* Left Column - Auto Update */}
                <Box
                  bg='#161616'
                  p={2}
                  borderRadius='md'
                  border='1px solid rgba(255, 255, 255, 0.08)'
                  display='flex'
                  flexDirection='column'
                  height='100%'
                >
                  <Text fontSize='sm' fontWeight='500' color='white' mb={1}>
                    Auto Update on Startup
                  </Text>
                  <Text
                    fontSize='xs'
                    color='whiteAlpha.600'
                    lineHeight='1.3'
                    flex='1'
                  >
                    Check for updates when app starts and prompt to install if
                    available.
                  </Text>
                  <Box
                    borderTop='1px solid rgba(255, 255, 255, 0.06)'
                    mt={3}
                    pt={3}
                  >
                    <Flex align='center' justify='flex-end' gap={2}>
                      <Text fontSize='xs' color='whiteAlpha.500'>
                        Enabled:
                      </Text>
                      <Checkbox
                        checked={autoUpdateEnabled}
                        onCheckedChange={e =>
                          setAutoUpdateEnabled(e.checked === true)
                        }
                        disabled={isLoading}
                        size='sm'
                      />
                    </Flex>
                  </Box>
                </Box>

                {/* Right Column - Version Information with Status */}
                <Box
                  bg='#161616'
                  p={2}
                  borderRadius='md'
                  border='1px solid rgba(255, 255, 255, 0.08)'
                  display='flex'
                  flexDirection='column'
                  height='100%'
                >
                  <Flex align='center' justify='space-between' mb={1}>
                    <Text fontSize='sm' fontWeight='500' color='white'>
                      Version Information
                    </Text>
                    <Button
                      size='2xs'
                      variant='outline'
                      onClick={
                        updateStatus === 'available'
                          ? installUpdate
                          : checkForUpdates
                      }
                      loading={isCheckingUpdates || isUpdating}
                      loadingText={isUpdating ? '...' : '...'}
                      disabled={isLoading}
                      height='18px'
                      fontSize='10px'
                      color='whiteAlpha.600'
                      borderColor='rgba(255, 255, 255, 0.1)'
                      _hover={{
                        borderColor: 'rgba(255, 255, 255, 0.2)',
                        bg: 'whiteAlpha.50',
                      }}
                      px={1.5}
                    >
                      <Box
                        as={updateStatus === 'available' ? Download : RefreshCw}
                        width='8px'
                        height='8px'
                        mr={0.5}
                      />
                      {updateStatus === 'available' ? 'Install' : 'Check'}
                    </Button>
                  </Flex>
                  <Text
                    fontSize='xs'
                    color='whiteAlpha.600'
                    lineHeight='1.3'
                    flex='1'
                  >
                    Current: {currentVersion || '...'}{updateStatus === 'available' ? ` → ${latestVersion}` : ''}
                  </Text>
                  <Box
                    borderTop='1px solid rgba(255, 255, 255, 0.06)'
                    mt={3}
                    pt={3}
                  >
                    <Flex align='center' gap={1.5}>
                      <Box
                        width='5px'
                        height='5px'
                        borderRadius='full'
                        bg={
                          updateStatus === 'checking'
                            ? 'blue.400'
                            : updateStatus === 'available'
                              ? 'green.400'
                              : updateStatus === 'up-to-date'
                                ? 'gray.400'
                                : updateStatus === 'error'
                                  ? 'red.400'
                                  : 'gray.500'
                        }
                      />
                      <Text fontSize='10px' color='whiteAlpha.500'>
                        {updateStatus === 'checking'
                          ? 'Checking...'
                          : updateStatus === 'available'
                            ? 'Update available'
                            : updateStatus === 'up-to-date'
                              ? 'Up to date'
                              : updateStatus === 'error'
                                ? 'Check failed'
                                : `Last: ${lastUpdateCheckDisplay}`}
                      </Text>
                    </Flex>
                  </Box>
                </Box>

                {/* Left Column - Log Retention */}
                <Box
                  bg='#161616'
                  p={2}
                  borderRadius='md'
                  border='1px solid rgba(255, 255, 255, 0.08)'
                  display='flex'
                  flexDirection='column'
                  height='100%'
                >
                  <Text fontSize='sm' fontWeight='500' color='white' mb={1}>
                    Log Retention
                  </Text>
                  <Text
                    fontSize='xs'
                    color='whiteAlpha.600'
                    lineHeight='1.3'
                    flex='1'
                  >
                    Auto-cleanup when both limits exceeded.
                  </Text>
                  <Box
                    borderTop='1px solid rgba(255, 255, 255, 0.06)'
                    mt={3}
                    pt={3}
                  >
                    <Flex align='center' justify='flex-end' gap={2} mb={1}>
                      <Text fontSize='xs' color='whiteAlpha.500'>
                        Max files:
                      </Text>
                      <Input
                        value={logRetentionCount}
                        onChange={handleLogRetentionCountChange}
                        size='xs'
                        width='45px'
                        height='22px'
                        bg='#111111'
                        border='1px solid rgba(255, 255, 255, 0.08)'
                        _hover={{ borderColor: 'rgba(255, 255, 255, 0.15)' }}
                        _focus={{ borderColor: 'blue.400', boxShadow: 'none' }}
                        color='white'
                        disabled={isLoading}
                        textAlign='center'
                        fontSize='xs'
                      />
                    </Flex>
                    <Flex align='center' justify='flex-end' gap={2}>
                      <Text fontSize='xs' color='whiteAlpha.500'>
                        Max days:
                      </Text>
                      <Input
                        value={logRetentionDays}
                        onChange={handleLogRetentionDaysChange}
                        size='xs'
                        width='45px'
                        height='22px'
                        bg='#111111'
                        border='1px solid rgba(255, 255, 255, 0.08)'
                        _hover={{ borderColor: 'rgba(255, 255, 255, 0.15)' }}
                        _focus={{ borderColor: 'blue.400', boxShadow: 'none' }}
                        color='white'
                        disabled={isLoading}
                        textAlign='center'
                        fontSize='xs'
                      />
                    </Flex>
                  </Box>
                </Box>

                {/* Right Column - Log Files */}
                <Box
                  bg='#161616'
                  p={2}
                  borderRadius='md'
                  border='1px solid rgba(255, 255, 255, 0.08)'
                  display='flex'
                  flexDirection='column'
                  height='100%'
                >
                  <Text fontSize='sm' fontWeight='500' color='white' mb={1}>
                    Log Files
                  </Text>
                  <Text
                    fontSize='xs'
                    color='whiteAlpha.600'
                    lineHeight='1.3'
                    flex='1'
                  >
                    {logFileCount} files • {formatFileSize(logTotalSize)} total
                  </Text>
                  <Box
                    borderTop='1px solid rgba(255, 255, 255, 0.06)'
                    mt={3}
                    pt={3}
                  >
                    <Flex align='center' justify='flex-end' mb={1}>
                      <Button
                        size='2xs'
                        variant='outline'
                        onClick={openLogsWindow}
                        disabled={isLoading}
                        height='18px'
                        fontSize='10px'
                        color='whiteAlpha.600'
                        borderColor='rgba(255, 255, 255, 0.1)'
                        _hover={{
                          borderColor: 'rgba(255, 255, 255, 0.2)',
                          bg: 'whiteAlpha.50',
                        }}
                        px={1.5}
                      >
                        <Box as={FileText} width='8px' height='8px' mr={0.5} />
                        View Logs
                      </Button>
                    </Flex>
                    <Flex align='center' justify='flex-end'>
                      <Button
                        size='2xs'
                        variant='outline'
                        onClick={cleanupLogs}
                        loading={isCleaningLogs}
                        loadingText='...'
                        disabled={isLoading}
                        height='18px'
                        fontSize='10px'
                        color='whiteAlpha.600'
                        borderColor='rgba(255, 255, 255, 0.1)'
                        _hover={{
                          borderColor: 'rgba(255, 255, 255, 0.2)',
                          bg: 'whiteAlpha.50',
                        }}
                        px={1.5}
                      >
                        <Box as={Trash2} width='8px' height='8px' mr={0.5} />
                        Purge Now
                      </Button>
                    </Flex>
                  </Box>
                </Box>

                {/* MCP Server Settings */}
                <McpServerSettings isLoading={isLoading} />
              </Box>
            </Stack>
          </Dialog.Body>

          <Dialog.Footer
            px={3}
            py={2}
            bg='#161616'
            borderTop='1px solid rgba(255, 255, 255, 0.05)'
          >
            <Flex justify='flex-end' gap={2} width='100%'>
              <Button
                variant='ghost'
                size='xs'
                onClick={onClose}
                disabled={isSaving}
                _hover={{ bg: 'whiteAlpha.50' }}
                color='gray.400'
                height='28px'
                fontSize='xs'
              >
                Cancel
              </Button>
              <Button
                size='xs'
                onClick={saveSettings}
                loading={isSaving}
                loadingText='Saving...'
                disabled={isLoading}
                bg='blue.500'
                color='white'
                _hover={{ bg: 'blue.600' }}
                _active={{ bg: 'blue.700' }}
                height='28px'
                fontSize='xs'
              >
                Save Settings
              </Button>
            </Flex>
          </Dialog.Footer>
        </Dialog.Content>
      </Dialog.Positioner>
    </Dialog.Root>
  )
}

export default SettingsModal
