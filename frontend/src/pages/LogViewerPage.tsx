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

export function LogViewerPage() {
  const [selectedFile, setSelectedFile] = useState<string | null>(null)
  const [autoRefresh, setAutoRefresh] = useState(false)
  const [expandedIds, setExpandedIds] = useState<Set<number>>(new Set())
  const [filter, setFilter] = useState<LogFilter>({
    levels: [],
    modules: [],
    searchText: '',
  })
  const deferredSearchText = useDeferredValue(filter.searchText)
  const { levels, modules } = filter
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
    mutationFn: (filename: string | null) => invoke('clear_logs', { filename }),
    onSuccess: async (_, filename) => {
      setExpandedIds(new Set())
      await queryClient.invalidateQueries({ queryKey: ['logs', filename] })
      toaster.success({
        title: 'Logs Cleared',
        description: 'Log file has been cleared',
        duration: 2000,
      })
    },
    meta: {
      errorToast: {
        title: 'Error',
        description: 'Failed to clear logs',
        duration: 3000,
      },
    },
  })
  const deleteLogFileMutation = useMutation({
    mutationFn: (filename: string) => invoke('delete_log_file', { filename }),
    onSuccess: async (_, filename) => {
      await queryClient.invalidateQueries({ queryKey: logFilesQueryKey })
      setSelectedFile(current => (current === filename ? null : current))
      toaster.success({
        title: 'File Deleted',
        description: 'Log file has been deleted',
        duration: 2000,
      })
    },
    meta: { errorToast: { title: 'Error', duration: 3000 } },
  })
  const { mutate: exportReport, isPending: isExportingReport } = useMutation({
    mutationFn: async () => {
      const report = await invoke<string>('generate_diagnostic_report')
      const url = URL.createObjectURL(
        new Blob([report], { type: 'application/json' }),
      )
      const anchor = document.createElement('a')
      anchor.href = url
      anchor.download = `kftray-report-${new Date().toISOString().slice(0, 10)}.json`
      anchor.click()
      URL.revokeObjectURL(url)
    },
    onSuccess: () => {
      toaster.success({
        title: 'Report Generated',
        description: 'Diagnostic report has been downloaded',
        duration: 3000,
      })
    },
    meta: {
      errorToast: {
        title: 'Error',
        description: 'Failed to generate report',
        duration: 3000,
      },
    },
  })
  const { mutate: openFolder } = useMutation({
    mutationFn: () => invoke('open_log_directory'),
    meta: {
      errorToast: {
        title: 'Error',
        description: 'Failed to open log directory',
        duration: 3000,
      },
    },
  })
  const { mutate: copyLogs } = useMutation({
    mutationFn: (text: string) => navigator.clipboard.writeText(text),
    onSuccess: () => {
      toaster.success({
        title: 'Copied',
        description: 'Logs copied to clipboard',
        duration: 2000,
      })
    },
    meta: {
      errorToast: {
        title: 'Error',
        description: 'Failed to copy logs',
        duration: 3000,
      },
    },
  })

  const entries = logsQuery.data?.entries ?? []
  const logInfo = logsQuery.data?.info
  const availableModules = useMemo(() => extractModules(entries), [entries])
  const filteredEntries = useMemo(
    () =>
      filterLogs(entries, {
        levels,
        modules,
        searchText: deferredSearchText,
      }),
    [deferredSearchText, entries, levels, modules],
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
    clearLogsMutation.mutate(selectedFile)
  }, [clearLogsMutation, selectedFile])
  const handleExport = useCallback(() => exportReport(), [exportReport])
  const handleOpenFolder = useCallback(() => openFolder(), [openFolder])
  const handleCopyLogs = useCallback(
    () => copyLogs(entries.map(entry => entry.raw).join('\n')),
    [copyLogs, entries],
  )
  const handleClose = useCallback(async () => {
    await appWindow.close()
  }, [appWindow])

  return (
    <Box
      display='flex'
      flexDirection='column'
      height='100vh'
      bg='bg.canvas'
      color='fg'
      overflow='hidden'
    >
      <Flex
        p={3}
        bg='bg.surface'
        borderBottom='1px solid'
        borderColor='border'
        align='center'
        justify='space-between'
        flexShrink={0}
        data-tauri-drag-region
      >
        <Flex align='center' gap={3}>
          <Text fontSize='sm' fontWeight='medium' color='fg'>
            Application Logs
          </Text>
          {logsQuery.isLoading && (
            <Text fontSize='xs' color='accent.fg'>
              Loading...
            </Text>
          )}
        </Flex>
        <Tooltip content='Close Window'>
          <Button
            size='xs'
            variant='ghost'
            onClick={handleClose}
            height='28px'
            width='28px'
            minWidth='28px'
            p={0}
            _hover={{ bg: 'bg.hover' }}
            aria-label='Close window'
          >
            <Box as={X} width='14px' height='14px' color='fg.muted' />
          </Button>
        </Tooltip>
      </Flex>
      <Box px={3} py={2} bg='bg.surface' flexShrink={0}>
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
          isExporting={isExportingReport}
        />
      </Box>
      <Box flex={1} bg='bg.deep' overflow='hidden' position='relative'>
        {logsQuery.isError ? (
          <Flex height='100%' align='center' justify='center'>
            <Text color='danger.fg'>{errorMessage(logsQuery.error)}</Text>
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
        bg='bg.surface'
        borderTop='1px solid'
        borderColor='border.subtle'
        align='center'
        justify='space-between'
        flexShrink={0}
      >
        <Text
          fontSize='xs'
          color='fg.faint'
          overflow='hidden'
          textOverflow='ellipsis'
          whiteSpace='nowrap'
          maxWidth='80%'
          title={logInfo?.log_path}
        >
          {logInfo?.log_path}
        </Text>
        <Text fontSize='xs' color='fg.faint'>
          {filteredEntries.length === entries.length
            ? `${entries.length} entries`
            : `${filteredEntries.length} / ${entries.length} entries`}
        </Text>
      </Flex>
    </Box>
  )
}
