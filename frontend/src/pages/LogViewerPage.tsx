import React, { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { X } from 'lucide-react'

import { Box, Flex, Text } from '@chakra-ui/react'
import { invoke } from '@tauri-apps/api/core'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'

import type {
  LogEntry,
  LogFileInfo,
  LogFilter,
  LogInfo,
} from '@/components/LogViewer'
import {
  AUTO_REFRESH_INTERVAL,
  DEFAULT_LOG_LINES,
  extractModules,
  filterLogs,
  LogFileSelector,
  LogViewerList,
  LogViewerToolbar,
} from '@/components/LogViewer'
import { Button } from '@/components/ui/button'
import { toaster } from '@/components/ui/toaster'
import { Tooltip } from '@/components/ui/tooltip'

const LogViewerPage: React.FC = () => {
  const [entries, setEntries] = useState<LogEntry[]>([])
  const [logInfo, setLogInfo] = useState<LogInfo | null>(null)
  const [logFiles, setLogFiles] = useState<LogFileInfo[]>([])
  const [selectedFile, setSelectedFile] = useState<string | null>(null)
  const [isLoading, setIsLoading] = useState(true)
  const [autoRefresh, setAutoRefresh] = useState(false)
  const [isExporting, setIsExporting] = useState(false)
  const [expandedIds, setExpandedIds] = useState<Set<number>>(new Set())
  const [filter, setFilter] = useState<LogFilter>({
    levels: [],
    modules: [],
    searchText: '',
  })

  const containerRef = useRef<HTMLDivElement>(null)
  const appWindow = getCurrentWebviewWindow()
  const isInitialLoadRef = useRef(true)

  const fetchLogFiles = useCallback(async () => {
    try {
      const files = await invoke<LogFileInfo[]>('list_log_files')

      setLogFiles(files)
    } catch (error) {
      console.error('Error fetching log files:', error)
    }
  }, [])

  const fetchLogs = useCallback(
    async (silent = false) => {
      if (!silent) {
        setIsLoading(true)
      }
      try {
        const [info, logEntries] = await Promise.all([
          invoke<LogInfo>('get_log_info', { filename: selectedFile }),
          invoke<LogEntry[]>('get_log_contents_json', {
            lines: DEFAULT_LOG_LINES,
            filename: selectedFile,
          }),
        ])

        setLogInfo(info)
        setEntries(logEntries)
      } catch (error) {
        console.error('Error fetching logs:', error)
      } finally {
        if (!silent) {
          setIsLoading(false)
        }
        isInitialLoadRef.current = false
      }
    },
    [selectedFile],
  )

  useEffect(() => {
    fetchLogFiles()
  }, [fetchLogFiles])

  useEffect(() => {
    fetchLogs(false)
  }, [fetchLogs])

  useEffect(() => {
    if (!autoRefresh || selectedFile !== null) {
      return
    }
    const interval = setInterval(() => fetchLogs(true), AUTO_REFRESH_INTERVAL)

    return () => clearInterval(interval)
  }, [autoRefresh, fetchLogs, selectedFile])

  const availableModules = useMemo(() => extractModules(entries), [entries])
  const filteredEntries = useMemo(
    () => filterLogs(entries, filter),
    [entries, filter],
  )

  const handleFileSelect = useCallback((filename: string | null) => {
    setSelectedFile(filename)
    setExpandedIds(new Set())
    if (filename !== null) {
      setAutoRefresh(false)
    }
  }, [])

  const handleDeleteFile = useCallback(
    async (filename: string) => {
      try {
        await invoke('delete_log_file', { filename })
        await fetchLogFiles()
        if (selectedFile === filename) {
          setSelectedFile(null)
        }
        toaster.success({
          title: 'File Deleted',
          description: 'Log file has been deleted',
          duration: 2000,
        })
      } catch (error) {
        console.error('Error deleting log file:', error)
        toaster.error({
          title: 'Error',
          description: String(error),
          duration: 3000,
        })
      }
    },
    [fetchLogFiles, selectedFile],
  )

  const handleToggleExpand = useCallback((id: number) => {
    setExpandedIds(prev => {
      const next = new Set(prev)

      if (next.has(id)) {
        next.delete(id)
      } else {
        next.add(id)
      }

      return next
    })
  }, [])

  const handleClear = useCallback(async () => {
    try {
      await invoke('clear_logs', { filename: selectedFile })
      await fetchLogs(false)
      setExpandedIds(new Set())
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
  }, [fetchLogs, selectedFile])

  const handleExport = useCallback(async () => {
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
  }, [])

  const handleOpenFolder = useCallback(async () => {
    try {
      await invoke('open_log_directory')
    } catch (error) {
      console.error('Error opening log directory:', error)
      toaster.error({
        title: 'Error',
        description: 'Failed to open log directory',
        duration: 3000,
      })
    }
  }, [])

  const handleCopyLogs = useCallback(async () => {
    try {
      const allRawLogs = entries.map(e => e.raw).join('\n')

      await navigator.clipboard.writeText(allRawLogs)
      toaster.success({
        title: 'Copied',
        description: 'Logs copied to clipboard',
        duration: 2000,
      })
    } catch (error) {
      console.error('Error copying logs:', error)
      toaster.error({
        title: 'Error',
        description: 'Failed to copy logs',
        duration: 3000,
      })
    }
  }, [entries])

  const handleClose = useCallback(async () => {
    await appWindow.close()
  }, [appWindow])

  return (
    <Box
      display='flex'
      flexDirection='column'
      height='100vh'
      bg='#111111'
      color='white'
      overflow='hidden'
    >
      <Flex
        p={3}
        bg='#161616'
        borderBottom='1px solid rgba(255, 255, 255, 0.08)'
        align='center'
        justify='space-between'
        flexShrink={0}
        data-tauri-drag-region
      >
        <Flex align='center' gap={3}>
          <Text fontSize='sm' fontWeight='medium' color='gray.100'>
            Application Logs
          </Text>
          {isLoading && (
            <Text fontSize='xs' color='blue.400'>
              Loading...
            </Text>
          )}
        </Flex>

        <Tooltip
          content='Close Window'
          portalled
          contentProps={{ zIndex: 100 }}
        >
          <Button
            size='xs'
            variant='ghost'
            onClick={handleClose}
            height='28px'
            width='28px'
            minWidth='28px'
            p={0}
            _hover={{ bg: 'whiteAlpha.100' }}
          >
            <Box as={X} width='14px' height='14px' color='whiteAlpha.700' />
          </Button>
        </Tooltip>
      </Flex>

      <Box px={3} py={2} bg='#141414' flexShrink={0}>
        <Flex align='center' gap={2} mb={2}>
          <LogFileSelector
            logFiles={logFiles}
            selectedFile={selectedFile}
            onFileSelect={handleFileSelect}
            onDeleteFile={handleDeleteFile}
            isLoading={isLoading}
          />
        </Flex>
        <LogViewerToolbar
          filter={filter}
          availableModules={availableModules}
          autoRefresh={autoRefresh}
          isFollowDisabled={selectedFile !== null}
          onFilterChange={setFilter}
          onAutoRefreshChange={setAutoRefresh}
          onClear={handleClear}
          onExport={handleExport}
          onCopy={handleCopyLogs}
          onOpenFolder={handleOpenFolder}
          isExporting={isExporting}
        />
      </Box>

      <Box
        ref={containerRef}
        flex={1}
        bg='#0a0a0a'
        overflow='hidden'
        position='relative'
      >
        <LogViewerList
          entries={filteredEntries}
          expandedIds={expandedIds}
          onToggleExpand={handleToggleExpand}
          searchText={filter.searchText}
          autoFollow={autoRefresh}
        />
      </Box>

      <Flex
        px={3}
        py={2}
        bg='#161616'
        borderTop='1px solid rgba(255, 255, 255, 0.05)'
        align='center'
        justify='space-between'
        flexShrink={0}
      >
        <Text
          fontSize='xs'
          color='whiteAlpha.400'
          overflow='hidden'
          textOverflow='ellipsis'
          whiteSpace='nowrap'
          maxWidth='80%'
          title={logInfo?.log_path}
        >
          {logInfo?.log_path}
        </Text>
        <Text fontSize='xs' color='whiteAlpha.400'>
          {filteredEntries.length === entries.length
            ? `${entries.length} entries`
            : `${filteredEntries.length} / ${entries.length} entries`}
        </Text>
      </Flex>
    </Box>
  )
}

export default LogViewerPage
