import React, { useCallback, useEffect, useState } from 'react'
import { FileText } from 'lucide-react'

import { Box, Dialog, Flex, Grid, Input, Stack, Text } from '@chakra-ui/react'
import { invoke } from '@tauri-apps/api/tauri'

import { Button } from '@/components/ui/button'
import { DialogCloseTrigger, DialogContent } from '@/components/ui/dialog'
import { Switch } from '@/components/ui/switch'
import { toaster } from '@/components/ui/toaster'

interface HttpLogsConfig {
  config_id: number
  enabled: boolean
  max_file_size: number
  retention_days: number
  auto_cleanup: boolean
}

interface HttpLogsConfigModalProps {
  configId: number
  isOpen: boolean
  onClose: () => void
  onSave?: () => void
}

const HttpLogsConfigModal: React.FC<HttpLogsConfigModalProps> = ({
  configId,
  isOpen,
  onClose,
  onSave,
}) => {
  const [config, setConfig] = useState<HttpLogsConfig>({
    config_id: configId,
    enabled: false,
    max_file_size: 10 * 1024 * 1024, // 10MB
    retention_days: 7,
    auto_cleanup: true,
  })
  const [isLoading, setIsLoading] = useState(false)
  const [isSaving, setIsSaving] = useState(false)

  const loadConfig = useCallback(async () => {
    setIsLoading(true)
    try {
      const httpConfig = await invoke<HttpLogsConfig>(
        'get_http_logs_config_cmd',
        {
          config_id: configId,
        },
      )

      setConfig(httpConfig)
    } catch (error) {
      console.error('Failed to load HTTP logs config:', error)
      toaster.error({
        title: 'Error',
        description: 'Failed to load HTTP logs configuration',
        duration: 3000,
      })
    } finally {
      setIsLoading(false)
    }
  }, [configId])

  useEffect(() => {
    if (isOpen) {
      loadConfig()
    }
  }, [isOpen, loadConfig])

  const handleSave = async () => {
    const mb = config.max_file_size / (1024 * 1024)
    const invalid =
      mb < 1 ||
      mb > 100 ||
      config.retention_days < 1 ||
      config.retention_days > 365


    if (invalid) {
      toaster.error({
        title: 'Invalid settings',
        description: 'Please fix highlighted fields before saving',
        duration: 3000,
      })
      
return
    }
    setIsSaving(true)
    try {
      await invoke('update_http_logs_config_cmd', { config })
      toaster.success({
        title: 'Settings Saved',
        description: 'HTTP logs configuration has been saved successfully',
        duration: 3000,
      })
      onSave?.()
      onClose()
    } catch (error) {
      console.error('Failed to save HTTP logs config:', error)
      toaster.error({
        title: 'Error',
        description: 'Failed to save HTTP logs configuration',
        duration: 3000,
      })
    } finally {
      setIsSaving(false)
    }
  }

  const formatFileSize = (bytes: number): string => {
    if (bytes >= 1024 * 1024) {
      return `${(bytes / (1024 * 1024)).toFixed(1)} MB`
    }
    if (bytes >= 1024) {
      return `${(bytes / 1024).toFixed(1)} KB`
    }

    return `${bytes} bytes`
  }

  const handleFileSizeChange = (e: React.ChangeEvent<HTMLInputElement>) => {
    const value = e.target.value
    const numValue = parseInt(value, 10)


    if (!Number.isNaN(numValue)) {
      const mb = Math.min(100, Math.max(1, numValue))


      setConfig(prev => ({ ...prev, max_file_size: mb * 1024 * 1024 }))
    }
  }

  const handleRetentionDaysChange = (
    e: React.ChangeEvent<HTMLInputElement>,
  ) => {
    const value = e.target.value
    const numValue = parseInt(value, 10)


    if (!Number.isNaN(numValue)) {
      const days = Math.min(365, Math.max(1, numValue))


      setConfig(prev => ({ ...prev, retention_days: days }))
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
        css={{
          '&::-webkit-scrollbar': { display: 'none' },
          '-ms-overflow-style': 'none',
          'scrollbar-width': 'none',
        }}
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
              as={FileText}
              width='14px'
              height='14px'
              color='blue.400'
              ml={2}
            />
            <Text fontSize='sm' fontWeight='600' color='white'>
              HTTP Logs Configuration
            </Text>
          </Flex>
        </Box>

        {/* Content */}
        <Box px={4} py={3}>
          {isLoading ? (
            <Box py={6} textAlign='center'>
              <Text color='whiteAlpha.600'>Loading configuration...</Text>
            </Box>
          ) : (
            <Stack gap={3}>
              <Grid templateColumns='1fr 1fr' gap={3}>
                {/* Enable HTTP Logs */}
                <Box
                  bg='#161616'
                  p={2.5}
                  borderRadius='md'
                  border='1px solid rgba(255, 255, 255, 0.08)'
                  height='fit-content'
                >
                  <Flex direction='column' gap={2}>
                    <Text fontSize='sm' fontWeight='500' color='white'>
                      Enable HTTP Logs
                    </Text>
                    <Text fontSize='xs' color='whiteAlpha.600' lineHeight='1.3'>
                      Enable HTTP request/response logging for this
                      configuration
                    </Text>
                    <Box alignSelf='flex-start'>
                      <Switch
                        checked={config.enabled}
                        onCheckedChange={details =>
                          setConfig(prev => ({
                            ...prev,
                            enabled: details.checked,
                          }))
                        }
                        disabled={isLoading}
                        colorPalette='blue'
                      />
                    </Box>
                  </Flex>
                </Box>

                {/* Automatic Cleanup */}
                <Box
                  bg='#161616'
                  p={2.5}
                  borderRadius='md'
                  border='1px solid rgba(255, 255, 255, 0.08)'
                  height='fit-content'
                >
                  <Flex direction='column' gap={2}>
                    <Text fontSize='sm' fontWeight='500' color='white'>
                      Automatic Cleanup
                    </Text>
                    <Text fontSize='xs' color='whiteAlpha.600' lineHeight='1.3'>
                      Automatically remove old log files based on retention
                      period
                    </Text>
                    <Box alignSelf='flex-start'>
                      <Switch
                        checked={config.auto_cleanup}
                        onCheckedChange={details =>
                          setConfig(prev => ({
                            ...prev,
                            auto_cleanup: details.checked,
                          }))
                        }
                        disabled={isLoading}
                        colorPalette='blue'
                      />
                    </Box>
                  </Flex>
                </Box>

                {/* Maximum File Size */}
                <Box
                  bg='#161616'
                  p={2.5}
                  borderRadius='md'
                  border='1px solid rgba(255, 255, 255, 0.08)'
                  height='fit-content'
                >
                  <Flex direction='column' gap={2}>
                    <Text fontSize='sm' fontWeight='500' color='white'>
                      Maximum File Size
                    </Text>
                    <Text fontSize='xs' color='whiteAlpha.600' lineHeight='1.3'>
                      Maximum file size before rotation. Current:{' '}
                      {formatFileSize(config.max_file_size)}
                    </Text>
                    <Flex align='center' gap={1}>
                      <Input
                        type='number'
                        value={(
                          config.max_file_size /
                          (1024 * 1024)
                        ).toString()}
                        onChange={handleFileSizeChange}
                        placeholder='10'
                        size='xs'
                        width='60px'
                        height='24px'
                        min={1}
                        max={100}
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
                      <Text fontSize='xs' color='whiteAlpha.600'>
                        MB
                      </Text>
                    </Flex>
                  </Flex>
                </Box>

                {/* Retention Period */}
                <Box
                  bg='#161616'
                  p={2.5}
                  borderRadius='md'
                  border='1px solid rgba(255, 255, 255, 0.08)'
                  height='fit-content'
                >
                  <Flex direction='column' gap={2}>
                    <Text fontSize='sm' fontWeight='500' color='white'>
                      Retention Period
                    </Text>
                    <Text fontSize='xs' color='whiteAlpha.600' lineHeight='1.3'>
                      Days to keep log files before cleanup
                    </Text>
                    <Flex align='center' gap={1}>
                      <Input
                        type='number'
                        value={config.retention_days.toString()}
                        onChange={handleRetentionDaysChange}
                        placeholder='7'
                        size='xs'
                        width='60px'
                        height='24px'
                        min={1}
                        max={365}
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
                      <Text fontSize='xs' color='whiteAlpha.600'>
                        days
                      </Text>
                    </Flex>
                  </Flex>
                </Box>
              </Grid>
            </Stack>
          )}
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
              onClick={handleSave}
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

export default HttpLogsConfigModal
