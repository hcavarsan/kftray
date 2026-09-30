import { useState } from 'react'
import { Download, RefreshCw } from 'lucide-react'

import { Box, Flex, Text } from '@chakra-ui/react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { app } from '@tauri-apps/api'
import { invoke } from '@tauri-apps/api/core'

import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { toaster } from '@/components/ui/toaster'

import { settingsQuery } from '../queries'
import {
  compactActionButtonProps,
  SettingCard,
  SettingRow,
} from '../SettingCard'

type UpdateStatus = 'idle' | 'checking' | 'available' | 'up-to-date' | 'error'

const STATUS_COLOR: Record<UpdateStatus, string> = {
  idle: 'gray.500',
  checking: 'blue.400',
  available: 'green.400',
  'up-to-date': 'gray.400',
  error: 'red.400',
}

const STATUS_LABEL: Record<Exclude<UpdateStatus, 'idle'>, string> = {
  checking: 'Checking...',
  available: 'Update available',
  'up-to-date': 'Up to date',
  error: 'Check failed',
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
  const [status, setStatus] = useState<UpdateStatus>('idle')
  const [isUpdating, setIsUpdating] = useState(false)

  const checkForUpdates = async () => {
    setStatus('checking')
    try {
      const versionInfo =
        await invoke<Record<string, string>>('get_version_info')

      setLatestVersion(versionInfo.latest_version || currentVersion || '')

      if (versionInfo.update_available === 'error') {
        setStatus('error')
        toaster.error({
          title: 'Update Check Failed',
          description: 'Failed to check for updates. Please try again later.',
          duration: 4000,
        })
      } else if (versionInfo.update_available === 'true') {
        setStatus('available')
        toaster.success({
          title: 'Update Available',
          description: `Version ${versionInfo.latest_version} is now available!`,
          duration: 4000,
        })
      } else {
        setStatus('up-to-date')
        toaster.success({
          title: 'Up to Date',
          description: 'You are running the latest version.',
          duration: 3000,
        })
      }
    } catch {
      setStatus('error')
      toaster.error({
        title: 'Update Check Failed',
        description: 'Failed to check for updates. Please try again later.',
        duration: 4000,
      })
    } finally {
      queryClient.invalidateQueries({ queryKey: settingsQuery.queryKey })
    }
  }

  const installUpdate = async () => {
    setIsUpdating(true)
    toaster.create({
      title: 'Installing Update',
      description:
        'The update is being downloaded and installed. App will restart automatically.',
      duration: 5000,
    })
    try {
      await invoke('install_update_silent')
    } catch {
      toaster.error({
        title: 'Update Failed',
        description: 'Failed to install the update. Please try again later.',
        duration: 4000,
      })
      setIsUpdating(false)
    }
  }

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
            onClick={updateAvailable ? installUpdate : checkForUpdates}
            loading={status === 'checking' || isUpdating}
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
          <Text fontSize='10px' color='whiteAlpha.500'>
            {status === 'idle'
              ? `Last: ${lastCheckLabel}`
              : STATUS_LABEL[status]}
          </Text>
        </Flex>
      </SettingCard>
    </>
  )
}
