import React, { useEffect, useRef, useState } from 'react'
import { Download, RefreshCw, Trash2 } from 'lucide-react'

import { Box, Dialog, Flex, Text } from '@chakra-ui/react'
import { invoke } from '@tauri-apps/api/core'

import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { DialogCloseTrigger } from '@/components/ui/dialog'
import { toaster } from '@/components/ui/toaster'

interface LogViewerModalProps {
  isOpen: boolean
  onClose: () => void
}

interface LogInfo {
  log_path: string
  log_size: number
  exists: boolean
}

const LogViewerModal: React.FC<LogViewerModalProps> = ({ isOpen, onClose }) => {
  const [logs, setLogs] = useState('')
  const [logInfo, setLogInfo] = useState<LogInfo | null>(null)
  const [isLoading, setIsLoading] = useState(false)
  const [autoRefresh, setAutoRefresh] = useState(false)
  const [isExporting, setIsExporting] = useState(false)
  const logsEndRef = useRef<HTMLDivElement>(null)

  const fetchLogs = async () => {
    setIsLoading(true)
    try {
      const [info, content] = await Promise.all([
        invoke<LogInfo>('get_log_info'),
        invoke<string>('get_log_contents', { lines: 500 }),
      ])


      setLogInfo(info)
      setLogs(content)
    } catch (error) {
      console.error('Error fetching logs:', error)
    } finally {
      setIsLoading(false)
    }
  }

  useEffect(() => {
    if (isOpen) {
      fetchLogs()
    }
  }, [isOpen])

  useEffect(() => {
    if (!autoRefresh || !isOpen) {
return
}
    const interval = setInterval(fetchLogs, 2000)


    
return () => clearInterval(interval)
  }, [autoRefresh, isOpen])

  useEffect(() => {
    if (autoRefresh && logsEndRef.current) {
      logsEndRef.current.scrollIntoView({ behavior: 'smooth' })
    }
  }, [logs, autoRefresh])

  const handleClear = async () => {
    try {
      await invoke('clear_logs')
      await fetchLogs()
      toaster.success({
        title: 'Logs Cleared',
        description: 'Log file has been cleared',
        duration: 2000,
      })
    } catch (error) {
      console.error('Error clearing logs:', error)
      toaster.error({
        title: 'Error',
        description: 'Failed to clear logs',
        duration: 3000,
      })
    }
  }

  const handleExport = async () => {
    setIsExporting(true)
    try {
      const report = await invoke<string>('generate_diagnostic_report')
      const blob = new Blob([report], { type: 'application/json' })
      const url = URL.createObjectURL(blob)
      const a = document.createElement('a')


      a.href = url
      a.download = `kftray-report-${new Date().toISOString().slice(0, 10)}.json`
      a.click()
      URL.revokeObjectURL(url)
      toaster.success({
        title: 'Report Generated',
        description: 'Diagnostic report has been downloaded',
        duration: 3000,
      })
    } catch (error) {
      console.error('Error generating report:', error)
      toaster.error({
        title: 'Error',
        description: 'Failed to generate report',
        duration: 3000,
      })
    } finally {
      setIsExporting(false)
    }
  }

  return (
    <Dialog.Root
      open={isOpen}
      onOpenChange={({ open }) => !open && onClose()}
      modal={true}
    >
      <Dialog.Backdrop bg='transparent' backdropFilter='blur(4px)' />
      <Dialog.Positioner>
        <Dialog.Content
          onClick={e => e.stopPropagation()}
          maxWidth='900px'
          width='95vw'
          height='85vh'
          bg='#111111'
          border='1px solid rgba(255, 255, 255, 0.08)'
          borderRadius='lg'
          overflow='hidden'
          display='flex'
          flexDirection='column'
        >
          <DialogCloseTrigger style={{ marginTop: '-4px' }} />

          <Dialog.Header
            p={3}
            bg='#161616'
            borderBottom='1px solid rgba(255, 255, 255, 0.05)'
            flexShrink={0}
          >
            <Flex justify='space-between' align='center' width='100%'>
              <Flex align='center' gap={2}>
                <Text fontSize='sm' fontWeight='medium' color='gray.100'>
                  Application Logs
                </Text>
                {logInfo && (
                  <Text fontSize='xs' color='whiteAlpha.500'>
                    ({(logInfo.log_size / 1024).toFixed(1)} KB)
                  </Text>
                )}
              </Flex>
              <Flex gap={2}>
                <Button
                  size='2xs'
                  variant='outline'
                  onClick={fetchLogs}
                  loading={isLoading}
                  height='24px'
                  fontSize='xs'
                  color='whiteAlpha.700'
                  borderColor='rgba(255, 255, 255, 0.15)'
                  _hover={{
                    borderColor: 'rgba(255, 255, 255, 0.3)',
                    bg: 'whiteAlpha.100',
                  }}
                  px={2}
                >
                  <Box as={RefreshCw} width='10px' height='10px' mr={1} />
                  Refresh
                </Button>
                <Button
                  size='2xs'
                  variant='outline'
                  onClick={handleClear}
                  height='24px'
                  fontSize='xs'
                  color='whiteAlpha.700'
                  borderColor='rgba(255, 255, 255, 0.15)'
                  _hover={{
                    borderColor: 'rgba(255, 255, 255, 0.3)',
                    bg: 'whiteAlpha.100',
                  }}
                  px={2}
                >
                  <Box as={Trash2} width='10px' height='10px' mr={1} />
                  Clear
                </Button>
                <Button
                  size='2xs'
                  variant='outline'
                  onClick={handleExport}
                  loading={isExporting}
                  height='24px'
                  fontSize='xs'
                  color='whiteAlpha.700'
                  borderColor='rgba(255, 255, 255, 0.15)'
                  _hover={{
                    borderColor: 'rgba(255, 255, 255, 0.3)',
                    bg: 'whiteAlpha.100',
                  }}
                  px={2}
                >
                  <Box as={Download} width='10px' height='10px' mr={1} />
                  Export Report
                </Button>
              </Flex>
            </Flex>
          </Dialog.Header>

          <Dialog.Body p={0} flex='1' overflow='hidden'>
            <Box
              height='100%'
              overflowY='auto'
              p={3}
              fontFamily='mono'
              fontSize='xs'
              whiteSpace='pre-wrap'
              wordBreak='break-word'
              color='whiteAlpha.700'
              bg='#0a0a0a'
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
              {logs || (
                <Text color='whiteAlpha.400' fontStyle='italic'>
                  No logs available
                </Text>
              )}
              <div ref={logsEndRef} />
            </Box>
          </Dialog.Body>

          <Dialog.Footer
            px={3}
            py={2}
            bg='#161616'
            borderTop='1px solid rgba(255, 255, 255, 0.05)'
            flexShrink={0}
          >
            <Flex justify='space-between' align='center' width='100%'>
              <Text
                fontSize='xs'
                color='whiteAlpha.400'
                overflow='hidden'
                textOverflow='ellipsis'
                whiteSpace='nowrap'
                maxWidth='60%'
                title={logInfo?.log_path}
              >
                {logInfo?.log_path}
              </Text>
              <Flex align='center' gap={3}>
                <Checkbox
                  checked={autoRefresh}
                  onCheckedChange={e => setAutoRefresh(e.checked === true)}
                  size='sm'
                >
                  <Text fontSize='xs' color='whiteAlpha.600'>
                    Auto-refresh
                  </Text>
                </Checkbox>
                <Button
                  variant='ghost'
                  size='xs'
                  onClick={onClose}
                  _hover={{ bg: 'whiteAlpha.50' }}
                  color='gray.400'
                  height='28px'
                  fontSize='xs'
                >
                  Close
                </Button>
              </Flex>
            </Flex>
          </Dialog.Footer>
        </Dialog.Content>
      </Dialog.Positioner>
    </Dialog.Root>
  )
}

export default LogViewerModal
