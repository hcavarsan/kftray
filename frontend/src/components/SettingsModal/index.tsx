import React, { useEffect, useState } from 'react'
import { Settings } from 'lucide-react'

import { Box, Dialog, Flex, Input, Stack, Text } from '@chakra-ui/react'
import { invoke } from '@tauri-apps/api/tauri'

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
  const [isLoading, setIsLoading] = useState(false)
  const [isSaving, setIsSaving] = useState(false)

  useEffect(() => {
    if (isOpen) {
      loadSettings()
    }
  }, [isOpen])

  const loadSettings = async () => {
    try {
      setIsLoading(true)
      const settings = await invoke<Record<string, string>>('get_settings')

      setDisconnectTimeout(settings.disconnect_timeout_minutes || '0')
      setNetworkMonitor(settings.network_monitor === 'true')
      setNetworkMonitorStatus(settings.network_monitor_status === 'true')
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
        maxWidth='420px'
        width='90vw'
        bg='#111111'
        border='1px solid rgba(255, 255, 255, 0.08)'
        borderRadius='lg'
        p={0}
        boxShadow='0 20px 25px -5px rgba(0, 0, 0, 0.3), 0 10px 10px -5px rgba(0, 0, 0, 0.1)'
      >
        <DialogCloseTrigger
          size='xs'
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
        <Box px={4} py={3}>
          <Stack gap={3}>
            {/* Port Forwarding Section */}
            <Box>
              {/* Auto-disconnect Timeout Setting */}
              <Box
                bg='#161616'
                p={2.5}
                borderRadius='md'
                border='1px solid rgba(255, 255, 255, 0.08)'
              >
                <Flex justify='space-between' align='flex-start' gap={4}>
                  <Box flex={1} maxWidth='60%'>
                    <Text fontSize='sm' fontWeight='500' color='white' mb={1}>
                      Auto-disconnect Timeout
                    </Text>
                    <Text fontSize='xs' color='whiteAlpha.600' lineHeight='1.4'>
                      Automatically disconnect port forwards after the specified
                      time (min). Set to 0 to disable.
                    </Text>
                  </Box>

                  <Box
                    minWidth='120px'
                    display='flex'
                    alignItems='flex-start'
                    justifyContent='flex-end'
                    pt={1}
                  >
                    <Input
                      value={disconnectTimeout}
                      onChange={handleTimeoutChange}
                      placeholder='0'
                      size='xs'
                      width='45px'
                      height='24px'
                      bg='#111111'
                      border='1px solid rgba(255, 255, 255, 0.08)'
                      _hover={{
                        borderColor: 'rgba(255, 255, 255, 0.15)',
                      }}
                      _focus={{
                        borderColor: 'blue.400',
                        boxShadow: 'none',
                      }}
                      color='white'
                      _placeholder={{ color: 'whiteAlpha.500' }}
                      disabled={isLoading}
                      textAlign='center'
                      fontSize='xs'
                    />
                  </Box>
                </Flex>
              </Box>

              {/* Network Monitor Setting */}
              <Box
                bg='#161616'
                p={2.5}
                borderRadius='md'
                border='1px solid rgba(255, 255, 255, 0.08)'
                mt={2}
              >
                <Flex justify='space-between' align='flex-start' gap={4}>
                  <Box flex={1} maxWidth='60%'>
                    <Flex align='center' gap={2} mb={1}>
                      <Text fontSize='sm' fontWeight='500' color='white'>
                        Network Monitor
                      </Text>
                      <Box
                        width='6px'
                        height='6px'
                        borderRadius='full'
                        bg={networkMonitorStatus ? 'green.400' : 'gray.500'}
                        title={networkMonitorStatus ? 'Running' : 'Stopped'}
                      />
                    </Flex>
                    <Text fontSize='xs' color='whiteAlpha.600' lineHeight='1.4'>
                      Monitor network connectivity and automatically reconnect
                      port forwards when network is restored.
                    </Text>
                  </Box>

                  <Box
                    minWidth='120px'
                    display='flex'
                    alignItems='flex-start'
                    justifyContent='flex-end'
                    pt={1}
                  >
                    <Checkbox
                      checked={networkMonitor}
                      onCheckedChange={e =>
                        setNetworkMonitor(e.checked === true)
                      }
                      disabled={isLoading}
                      mr={3}
                    />
                  </Box>
                </Flex>
              </Box>
            </Box>

            {/* Future sections can be added here */}
            {/* Example structure for future settings:
            <Box>
              <Text fontSize='sm' fontWeight='600' color='white' mb={2}>
                Application
              </Text>
              <Stack gap={2}>
                <Box bg='#161616' p={2.5} borderRadius='md' border='1px solid rgba(255, 255, 255, 0.08)'>
                  // Another setting item
                </Box>
              </Stack>
            </Box>
            */}
          </Stack>
        </Box>

        {/* Footer */}
        <Box
          bg='#161616'
          px={4}
          py={3}
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
        </Box>
      </DialogContent>
    </Dialog.Root>
  )
}

export default SettingsModal
