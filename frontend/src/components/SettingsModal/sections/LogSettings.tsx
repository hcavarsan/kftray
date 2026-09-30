import { FileText, Trash2 } from 'lucide-react'

import { Box, Flex, Input, Stack } from '@chakra-ui/react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { invoke } from '@tauri-apps/api/core'

import { logFilesQuery } from '@/components/LogViewer'
import { Button } from '@/components/ui/button'
import { toaster } from '@/components/ui/toaster'
import { formatBytes } from '@/lib/format'

import {
  compactActionButtonProps,
  compactInputProps,
  isDigitsUpTo,
  LOAD_FAILED_NOTE,
  SettingCard,
  SettingRow,
} from '../SettingCard'

interface LogSettingsProps {
  retentionCount: string
  onRetentionCountChange: (value: string) => void
  retentionDays: string
  onRetentionDaysChange: (value: string) => void
  disabled: boolean
}

export function LogSettings({
  retentionCount,
  onRetentionCountChange,
  retentionDays,
  onRetentionDaysChange,
  disabled,
}: LogSettingsProps) {
  const queryClient = useQueryClient()
  const { data: logFiles = [] } = useQuery(logFilesQuery)

  const totalSize = logFiles.reduce((acc, f) => acc + f.size, 0)

  const cleanupMutation = useMutation({
    mutationFn: () => invoke<number>('cleanup_old_logs'),
    onSuccess: async deleted => {
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
    },
    meta: { errorToast: { title: 'Cleanup Failed', duration: 4000 } },
  })

  const openLogsMutation = useMutation({
    mutationFn: () => invoke('open_log_viewer_window_cmd'),
    meta: {
      errorToast: {
        title: 'Error',
        description: 'Failed to open log viewer',
        duration: 3000,
      },
    },
  })

  return (
    <>
      <SettingCard
        title='Log Retention'
        description='Auto-cleanup when both limits exceeded.'
        opacity={disabled ? 0.5 : undefined}
        note={disabled ? LOAD_FAILED_NOTE : undefined}
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
              disabled={disabled}
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
              disabled={disabled}
            />
          </SettingRow>
        </Stack>
      </SettingCard>

      <SettingCard
        title='Log Files'
        description={`${logFiles.length} files • ${formatBytes(totalSize)} total`}
      >
        <Flex direction='column' align='flex-end' gap={1}>
          <Button
            {...compactActionButtonProps}
            onClick={() => openLogsMutation.mutate()}
          >
            <Box as={FileText} width='8px' height='8px' mr={0.5} />
            View Logs
          </Button>
          <Button
            {...compactActionButtonProps}
            onClick={() => cleanupMutation.mutate()}
            loading={cleanupMutation.isPending}
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
