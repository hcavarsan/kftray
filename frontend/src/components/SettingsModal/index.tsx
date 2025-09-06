import React, { useEffect, useState } from 'react'
import { Download, RefreshCw, Settings } from 'lucide-react'

import { Box, Dialog, Flex, Input, Stack, Text } from '@chakra-ui/react'
import { app } from '@tauri-apps/api'
import { invoke } from '@tauri-apps/api/core'

import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { DialogCloseTrigger, DialogContent } from '@/components/ui/dialog'
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
  const [lastUpdateCheck, setLastUpdateCheck] = useState<string>('Never')
  const [currentVersion, setCurrentVersion] = useState<string>('')
  const [latestVersion, setLatestVersion] = useState<string>('')
  const [updateStatus, setUpdateStatus] = useState<
    'idle' | 'checking' | 'available' | 'up-to-date' | 'error'
  >('idle')
  const [isLoading, setIsLoading] = useState(false)
  const [isSaving, setIsSaving] = useState(false)
  const [isCheckingUpdates, setIsCheckingUpdates] = useState(false)
  const [isUpdating, setIsUpdating] = useState(false)

  useEffect(() => {
    if (isOpen) {
      loadSettings()
      loadVersionInfo()
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

        setLastUpdateCheck(
          date.toLocaleDateString() + ' at ' + date.toLocaleTimeString(),
        )
      } else {
        setLastUpdateCheck('Never')
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

  const saveSettings = async () => {
    try {
      setIsSaving(true)
      const timeoutValue = parseInt(disconnectTimeout, 10)

      if (isNaN(timeoutValue) || timeoutValue < 0) {
        toaster.error({
          title: 'Invalid Input',
          description: 'Please enter a valid number (0 or greater)',
          duration: 3000,
        })

        return
      }

      await invoke('update_disconnect_timeout', { minutes: timeoutValue })
      await invoke('update_network_monitor', { enabled: networkMonitor })
      await invoke('update_auto_update_enabled', { enabled: autoUpdateEnabled })

      // Reload settings to get updated status
      await loadSettings()

      toaster.success({
        title: 'Settings Saved',
        description: 'All settings have been saved successfully',
        duration: 3000,
      })

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

    // Only allow numbers

    if (value === '' || /^\d+$/.test(value)) {
      setDisconnectTimeout(value)
    }
  }

  return (
    <Dialog.Root open={isOpen} onOpenChange={({ open }) => !open && onClose()}>
      <DialogContent
        maxWidth='450px'
        maxHeight='100vh'
        minHeight='300px'
        width='90vw'
        bg='#111111'
        border='1px solid rgba(255, 255, 255, 0.08)'
        borderRadius='lg'
        p={0}
        overflow='hidden'
        boxShadow='0 20px 25px -5px rgba(0, 0, 0, 0.3), 0 10px 10px -5px rgba(0, 0, 0, 0.1)'
        mt='10'
      >
        <DialogCloseTrigger
          style={{
            marginTop: '-4px',
          }}
        />

        {/* Header */}
        <Box
          bg='#161616'
          px={4}
          py={3}
          borderBottom='1px solid rgba(255, 255, 255, 0.08)'
          borderTopRadius='lg'
        >
          <Flex align='center' gap={3}>
            <Box
              as={Settings}
              width='14px'
              height='14px'
              color='blue.400'
              ml={2}
            />
            <Text fontSize='sm' fontWeight='600' color='white'>
              Settings
            </Text>
          </Flex>
        </Box>

        {/* Content */}
        <Box px={2} py={2} maxHeight='calc(85vh - 100px)'>
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
                  mb={2}
                  flex='1'
                >
                  Disconnect port forwards after specified time (min). Set to 0
                  to disable.
                </Text>
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
                  mb={2}
                  flex='1'
                >
                  Monitor connectivity and reconnect port forwards when network
                  is restored.
                </Text>
                <Flex align='center' justify='flex-end' gap={2}>
                  <Text fontSize='xs' color='whiteAlpha.500'>
                    Enabled:
                  </Text>
                  <Checkbox
                    checked={networkMonitor}
                    onCheckedChange={e => setNetworkMonitor(e.checked === true)}
                    disabled={isLoading}
                    size='sm'
                  />
                </Flex>
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
                  mb={2}
                  flex='1'
                >
                  Check for updates when app starts and prompt to install if
                  available.
                </Text>
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

              {/* Right Column - Version Information */}
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
                  Version Information
                </Text>
                <Box flex='1'>
                  <Text fontSize='xs' color='whiteAlpha.600' mb={0.5}>
                    Current: {currentVersion || 'Loading...'}
                  </Text>
                  {updateStatus === 'available' && (
                    <Text fontSize='xs' color='whiteAlpha.600' mb={1}>
                      Latest: {latestVersion}
                    </Text>
                  )}
                </Box>

                <Flex justify='flex-end' gap={1.5}>
                  <Button
                    size='2xs'
                    variant='outline'
                    onClick={
                      updateStatus === 'available'
                        ? installUpdate
                        : checkForUpdates
                    }
                    loading={isCheckingUpdates || isUpdating}
                    loadingText={isUpdating ? 'Installing...' : 'Checking...'}
                    disabled={isLoading}
                    height='20px'
                    fontSize='xs'
                    color='whiteAlpha.700'
                    borderColor='rgba(255, 255, 255, 0.15)'
                    _hover={{
                      borderColor: 'rgba(255, 255, 255, 0.3)',
                      bg: 'whiteAlpha.100',
                    }}
                    px={2}
                  >
                    <Box
                      as={updateStatus === 'available' ? Download : RefreshCw}
                      width='8px'
                      height='8px'
                      mr={1}
                    />
                    {updateStatus === 'available' ? 'Install' : 'Check'}
                  </Button>
                </Flex>
              </Box>
            </Box>

            {/* Update Status Indicator - Always Visible */}
            <Box
              bg='#161616'
              p={2}
              borderRadius='md'
              border='1px solid rgba(255, 255, 255, 0.08)'
              gridColumn='1 / -1'
            >
              <Flex align='center' gap={2}>
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
                <Text fontSize='xs' color='whiteAlpha.600'>
                  {updateStatus === 'checking'
                    ? 'Checking for updates...'
                    : updateStatus === 'available'
                      ? `Update to ${latestVersion} available`
                      : updateStatus === 'up-to-date'
                        ? `Application is up to date - Last check: ${lastUpdateCheck}`
                        : updateStatus === 'error'
                          ? 'Update check failed'
                          : 'Click "Check" to verify for updates'}
                </Text>
              </Flex>
            </Box>
          </Stack>
        </Box>

        {/* Footer */}
        <Box
          bg='#161616'
          px={4}
          py={2}
          borderTop='1px solid rgba(255, 255, 255, 0.08)'
          borderBottomRadius='lg'
        >
          <Flex justify='flex-end' gap={2}>
            <Button
              variant='ghost'
              size='xs'
              onClick={onClose}
              disabled={isSaving}
              _hover={{ bg: 'whiteAlpha.100' }}
              color='whiteAlpha.700'
              height='28px'
              fontSize='sm'
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
              fontSize='sm'
            >
              Save Settings
            </Button>
          </Flex>
        </Box>
      </DialogContent>
    </Dialog.Root>
  )
}

export default SettingsModal
