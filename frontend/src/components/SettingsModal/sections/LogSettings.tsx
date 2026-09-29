import { useState } from 'react'
import { FileText, Trash2 } from 'lucide-react'

import { Box, Flex, Input, Stack } from '@chakra-ui/react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { invoke } from '@tauri-apps/api/core'

import { Button } from '@/components/ui/button'
import { toaster } from '@/components/ui/toaster'
import { errorMessage } from '@/lib/errors'

import { logFilesQuery } from '../queries'
import {
  compactActionButtonProps,
  compactInputProps,
  isDigitsUpTo,
  SettingCard,
  SettingRow,
} from '../SettingCard'

interface LogSettingsProps {
  retentionCount: string
  onRetentionCountChange: (value: string) => void
  retentionDays: string
  onRetentionDaysChange: (value: string) => void
}

export function LogSettings({
  retentionCount,
  onRetentionCountChange,
  retentionDays,
  onRetentionDaysChange,
}: LogSettingsProps) {
  const queryClient = useQueryClient()
  const { data: logFiles = [] } = useQuery(logFilesQuery)
  const [isCleaning, setIsCleaning] = useState(false)

  const totalSize = logFiles.reduce((acc, f) => acc + f.size, 0)
  const totalSizeLabel =
    totalSize < 1024
      ? `${totalSize} B`
      : totalSize < 1024 * 1024
        ? `${(totalSize / 1024).toFixed(1)} KB`
        : `${(totalSize / (1024 * 1024)).toFixed(1)} MB`

  const cleanupLogs = async () => {
    setIsCleaning(true)
    try {
      const deleted = await invoke<number>('cleanup_old_logs')

      await queryClient.invalidateQueries({ queryKey: logFilesQuery.queryKey })

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
      toaster.error({
        title: 'Cleanup Failed',
        description: errorMessage(error),
        duration: 4000,
      })
    } finally {
      setIsCleaning(false)
    }
  }

  const openLogsWindow = async () => {
    try {
      await invoke('open_log_viewer_window_cmd')
    } catch (error) {
      toaster.error({
        title: 'Failed to open log viewer',
        description: errorMessage(error),
        duration: 3000,
      })
    }
  }

  return (
    <>
      <SettingCard
        title='Log Retention'
        description='Auto-cleanup when both limits exceeded.'
      >
        <Stack gap={1}>
          <SettingRow label='Max files:'>
            <Input
              {...compactInputProps}
              value={retentionCount}
              onChange={e => {
                if (isDigitsUpTo(e.target.value, 100)) {
                  onRetentionCountChange(e.target.value)
                }
              }}
              width='45px'
            />
          </SettingRow>
          <SettingRow label='Max days:'>
            <Input
              {...compactInputProps}
              value={retentionDays}
              onChange={e => {
                if (isDigitsUpTo(e.target.value, 365)) {
                  onRetentionDaysChange(e.target.value)
                }
              }}
              width='45px'
            />
          </SettingRow>
        </Stack>
      </SettingCard>

      <SettingCard
        title='Log Files'
        description={`${logFiles.length} files • ${totalSizeLabel} total`}
      >
        <Flex direction='column' align='flex-end' gap={1}>
          <Button {...compactActionButtonProps} onClick={openLogsWindow}>
            <Box as={FileText} width='8px' height='8px' mr={0.5} />
            View Logs
          </Button>
          <Button
            {...compactActionButtonProps}
            onClick={cleanupLogs}
            loading={isCleaning}
            loadingText='...'
          >
            <Box as={Trash2} width='8px' height='8px' mr={0.5} />
            Purge Now
          </Button>
        </Flex>
      </SettingCard>
    </>
  )
}
