import { useState } from 'react'
import { Download, RefreshCw } from 'lucide-react'

import { Box, Flex, Text } from '@chakra-ui/react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { app } from '@tauri-apps/api'

import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { toaster } from '@/components/ui/toaster'
import { invoke } from '@/lib/tauri'

import { settingsQuery } from '../queries'
import {
  compactActionButtonProps,
  SettingCard,
  SettingRow,
} from '../SettingCard'

type UpdateStatus = 'idle' | 'checking' | 'available' | 'up-to-date' | 'error'

const STATUS_COLOR: Record<UpdateStatus, string> = {
  idle: 'fg.subtle',
  checking: 'accent.fg',
  available: 'success.fg',
  'up-to-date': 'fg.muted',
  error: 'danger.fg',
}

const STATUS_LABEL: Record<Exclude<UpdateStatus, 'idle'>, string> = {
  checking: 'Checking...',
  available: 'Update available',
  'up-to-date': 'Up to date',
  error: 'Check failed',
}

const CHECK_FAILED_TOAST = {
  title: 'Update Check Failed',
  description: 'Failed to check for updates. Please try again later.',
  duration: 4000,
}

interface UpdateSettingsProps {
  autoUpdateEnabled: boolean
  onAutoUpdateEnabledChange: (enabled: boolean) => void
  lastUpdateCheck: string | undefined
}

export function UpdateSettings({
  autoUpdateEnabled,
  onAutoUpdateEnabledChange,
  lastUpdateCheck,
}: UpdateSettingsProps) {
  const queryClient = useQueryClient()
  const { data: currentVersion } = useQuery({
    queryKey: ['app-version'],
    queryFn: () => app.getVersion(),
  })
  const [latestVersion, setLatestVersion] = useState('')
  const [checkResult, setCheckResult] = useState<UpdateStatus>('idle')

  const checkMutation = useMutation({
    mutationFn: () => invoke<Record<string, string>>('get_version_info'),
    onSuccess: versionInfo => {
      setLatestVersion(versionInfo.latest_version || currentVersion || '')

      if (versionInfo.update_available === 'true') {
        setCheckResult('available')
        toaster.success({
          title: 'Update Available',
          description: `Version ${versionInfo.latest_version} is now available!`,
          duration: 4000,
        })
      } else if (versionInfo.update_available === 'false') {
        setCheckResult('up-to-date')
        toaster.success({
          title: 'Up to Date',
          description: 'You are running the latest version.',
          duration: 3000,
        })
      } else {
        setCheckResult('error')
        toaster.error(CHECK_FAILED_TOAST)
      }
    },
    onError: () => setCheckResult('error'),
    onSettled: () => {
      queryClient.invalidateQueries({ queryKey: settingsQuery.queryKey })
    },
    meta: { errorToast: CHECK_FAILED_TOAST },
  })

  const installMutation = useMutation({
    mutationFn: () => invoke('install_update_silent'),
    onMutate: () => {
      toaster.create({
        title: 'Installing Update',
        description:
          'The update is being downloaded and installed. App will restart automatically.',
        duration: 5000,
      })
    },
    meta: {
      errorToast: {
        title: 'Update Failed',
        description: 'Failed to install the update. Please try again later.',
        duration: 4000,
      },
    },
  })

  const status: UpdateStatus = checkMutation.isPending
    ? 'checking'
    : checkResult

  const lastCheckSeconds = parseInt(lastUpdateCheck || '0', 10)
  const lastCheckDate = new Date(lastCheckSeconds * 1000)
  const lastCheckLabel =
    lastCheckSeconds > 0
      ? `${lastCheckDate.toLocaleDateString()} at ${lastCheckDate.toLocaleTimeString()}`
      : 'Never'
  const updateAvailable = status === 'available'

  return (
    <>
      <SettingCard
        title='Auto Update on Startup'
        description='Check for updates when app starts and prompt to install if available.'
      >
        <SettingRow label='Enabled:'>
          <Checkbox
            checked={autoUpdateEnabled}
            onCheckedChange={e => onAutoUpdateEnabledChange(e.checked === true)}
            size='sm'
          />
        </SettingRow>
      </SettingCard>

      <SettingCard
        title='Version Information'
        description={`Current: ${currentVersion || '...'}${updateAvailable ? ` → ${latestVersion}` : ''}`}
        action={
          <Button
            {...compactActionButtonProps}
            onClick={() =>
              updateAvailable
                ? installMutation.mutate()
                : checkMutation.mutate()
            }
            loading={checkMutation.isPending || installMutation.isPending}
            loadingText='...'
          >
            <Box
              as={updateAvailable ? Download : RefreshCw}
              width='8px'
              height='8px'
              mr={0.5}
            />
            {updateAvailable ? 'Install' : 'Check'}
          </Button>
        }
      >
        <Flex align='center' gap={1.5}>
          <Box
            width='5px'
            height='5px'
            borderRadius='full'
            bg={STATUS_COLOR[status]}
          />
          <Text fontSize='10px' color='fg.subtle'>
            {status === 'idle'
              ? `Last: ${lastCheckLabel}`
              : STATUS_LABEL[status]}
          </Text>
        </Flex>
      </SettingCard>
    </>
  )
}
