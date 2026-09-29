import { useCallback, useDeferredValue, useMemo, useState } from 'react'
import { X } from 'lucide-react'

import { Box, Flex, Text } from '@chakra-ui/react'
import {
  keepPreviousData,
  useMutation,
  useQuery,
  useQueryClient,
} from '@tanstack/react-query'
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
  normalizeLogEntries,
} from '@/components/LogViewer'
import { Button } from '@/components/ui/button'
import { toaster } from '@/components/ui/toaster'
import { Tooltip } from '@/components/ui/tooltip'
import { errorMessage } from '@/lib/errors'

interface LogData {
  entries: LogEntry[]
  info: LogInfo
}

const logFilesQueryKey = ['log-files'] as const

function LogViewerPage() {
  const [selectedFile, setSelectedFile] = useState<string | null>(null)
  const [autoRefresh, setAutoRefresh] = useState(false)
  const [isExporting, setIsExporting] = useState(false)
  const [expandedIds, setExpandedIds] = useState<Set<number>>(new Set())
  const [filter, setFilter] = useState<LogFilter>({
    levels: [],
    modules: [],
    searchText: '',
  })
  const deferredSearchText = useDeferredValue(filter.searchText)
  const queryClient = useQueryClient()
  const appWindow = getCurrentWebviewWindow()

  const logFilesQuery = useQuery({
    queryKey: logFilesQueryKey,
    queryFn: () => invoke<LogFileInfo[]>('list_log_files'),
  })
  const logsQuery = useQuery({
    queryKey: ['logs', selectedFile],
    queryFn: async (): Promise<LogData> => {
      const [info, entries] = await Promise.all([
        invoke<LogInfo>('get_log_info', { filename: selectedFile }),
        invoke<LogEntry[]>('get_log_contents_json', {
          lines: DEFAULT_LOG_LINES,
          filename: selectedFile,
        }),
      ])

      return { info, entries: normalizeLogEntries(entries) }
    },
    placeholderData: keepPreviousData,
    refetchInterval:
      autoRefresh && selectedFile === null ? AUTO_REFRESH_INTERVAL : false,
  })
  const clearLogsMutation = useMutation({
    mutationFn: () => invoke('clear_logs', { filename: selectedFile }),
    onSuccess: async () => {
      setExpandedIds(new Set())
      await queryClient.invalidateQueries({ queryKey: ['logs', selectedFile] })
      toaster.success({
        title: 'Logs Cleared',
        description: 'Log file has been cleared',
        duration: 2000,
      })
    },
    onError: error => {
      toaster.error({ title: 'Error', description: errorMessage(error) })
    },
  })
  const deleteLogFileMutation = useMutation({
    mutationFn: (filename: string) => invoke('delete_log_file', { filename }),
    onSuccess: async (_, filename) => {
      await queryClient.invalidateQueries({ queryKey: logFilesQueryKey })
      if (selectedFile === filename) {
        setSelectedFile(null)
      }
      toaster.success({
        title: 'File Deleted',
        description: 'Log file has been deleted',
        duration: 2000,
      })
    },
    onError: error => {
      toaster.error({ title: 'Error', description: errorMessage(error) })
    },
  })

  const entries = logsQuery.data?.entries ?? []
  const logInfo = logsQuery.data?.info
  const availableModules = useMemo(() => extractModules(entries), [entries])
  const filteredEntries = useMemo(
    () => filterLogs(entries, { ...filter, searchText: deferredSearchText }),
    [deferredSearchText, entries, filter],
  )

  const handleFileSelect = useCallback((filename: string | null) => {
    setSelectedFile(filename)
    setExpandedIds(new Set())
    if (filename !== null) {
      setAutoRefresh(false)
    }
  }, [])
  const handleDeleteFile = useCallback(
    (filename: string) => deleteLogFileMutation.mutate(filename),
    [deleteLogFileMutation],
  )
  const handleToggleExpand = useCallback((id: number) => {
    setExpandedIds(previousIds => {
      const nextIds = new Set(previousIds)
      if (nextIds.has(id)) {
        nextIds.delete(id)
      } else {
        nextIds.add(id)
      }
      return nextIds
    })
  }, [])
  const handleClear = useCallback(() => {
    clearLogsMutation.mutate()
  }, [clearLogsMutation])
  const handleExport = useCallback(async () => {
    setIsExporting(true)
    try {
      const report = await invoke<string>('generate_diagnostic_report')
      const url = URL.createObjectURL(
        new Blob([report], { type: 'application/json' }),
      )
      const anchor = document.createElement('a')
      anchor.href = url
      anchor.download = `kftray-report-${new Date().toISOString().slice(0, 10)}.json`
      anchor.click()
      URL.revokeObjectURL(url)
      toaster.success({
        title: 'Report Generated',
        description: 'Diagnostic report has been downloaded',
        duration: 3000,
      })
    } catch (error) {
      toaster.error({ title: 'Error', description: errorMessage(error) })
    } finally {
      setIsExporting(false)
    }
  }, [])
  const handleOpenFolder = useCallback(async () => {
    try {
      await invoke('open_log_directory')
    } catch (error) {
      toaster.error({ title: 'Error', description: errorMessage(error) })
    }
  }, [])
  const handleCopyLogs = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(
        entries.map(entry => entry.raw).join('\n'),
      )
      toaster.success({
        title: 'Copied',
        description: 'Logs copied to clipboard',
        duration: 2000,
      })
    } catch (error) {
      toaster.error({ title: 'Error', description: errorMessage(error) })
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
      bg='app.bg'
      color='white'
      overflow='hidden'
    >
      <Flex
        p={3}
        bg='app.panel'
        borderBottom='1px solid'
        borderColor='app.border'
        align='center'
        justify='space-between'
        flexShrink={0}
        data-tauri-drag-region
      >
        <Flex align='center' gap={3}>
          <Text fontSize='sm' fontWeight='medium' color='gray.100'>
            Application Logs
          </Text>
          {logsQuery.isLoading && (
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
            aria-label='Close window'
          >
            <Box as={X} width='14px' height='14px' color='whiteAlpha.700' />
          </Button>
        </Tooltip>
      </Flex>
      <Box px={3} py={2} bg='app.sunken' flexShrink={0}>
        <Flex align='center' gap={2} mb={2}>
          <LogFileSelector
            logFiles={logFilesQuery.data ?? []}
            selectedFile={selectedFile}
            onFileSelect={handleFileSelect}
            onDeleteFile={handleDeleteFile}
            isLoading={
              logFilesQuery.isLoading || deleteLogFileMutation.isPending
            }
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
      <Box flex={1} bg='app.deep' overflow='hidden' position='relative'>
        {logsQuery.isError ? (
          <Flex height='100%' align='center' justify='center'>
            <Text color='red.300'>{errorMessage(logsQuery.error)}</Text>
          </Flex>
        ) : (
          <LogViewerList
            entries={filteredEntries}
            expandedIds={expandedIds}
            onToggleExpand={handleToggleExpand}
            searchText={deferredSearchText}
            autoFollow={autoRefresh}
          />
        )}
      </Box>
      <Flex
        px={3}
        py={2}
        bg='app.panel'
        borderTop='1px solid'
        borderColor='app.subtle'
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
