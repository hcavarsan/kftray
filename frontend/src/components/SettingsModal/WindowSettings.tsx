import { useState } from 'react'
import { Check, ChevronDown, LocateFixed } from 'lucide-react'

import { Box, Flex } from '@chakra-ui/react'
import { invoke } from '@tauri-apps/api/core'

import { Button } from '@/components/ui/button'
import {
  MenuContent,
  MenuItem,
  MenuRoot,
  MenuTrigger,
} from '@/components/ui/menu'
import { toaster } from '@/components/ui/toaster'

import {
  compactActionButtonProps,
  SettingCard,
  SettingRow,
} from './SettingCard'
import type { AppMode } from './types'

const APP_MODES: { id: AppMode; label: string }[] = [
  { id: 'tray', label: 'Tray' },
  { id: 'window', label: 'Window' },
]

interface WindowSettingsProps {
  appMode: AppMode
  onAppModeChange: (mode: AppMode) => void
}

export function WindowSettings({
  appMode,
  onAppModeChange,
}: WindowSettingsProps) {
  const [isResettingPosition, setIsResettingPosition] = useState(false)

  const resetPosition = async () => {
    setIsResettingPosition(true)
    try {
      await invoke('reset_window_position_cmd')
    } catch {
      toaster.error({
        title: 'Error',
        description: 'Failed to reset window position',
        duration: 3000,
      })
    } finally {
      setIsResettingPosition(false)
    }
  }

  return (
    <>
      <SettingCard
        title='App Mode'
        description='Tray opens from the tray icon and hides when it loses focus, falling back to Window when no tray is available. Window runs as a regular app.'
      >
        <SettingRow label='Mode:'>
          <MenuRoot positioning={{ sameWidth: true }}>
            <MenuTrigger asChild>
              <Button
                size='xs'
                width='110px'
                height='22px'
                px={2}
                justifyContent='space-between'
                bg='app.bg'
                border='1px solid'
                borderColor='app.border'
                _hover={{ borderColor: 'app.borderStrong' }}
                _expanded={{ borderColor: 'blue.400' }}
                color='white'
                fontSize='xs'
                fontWeight='normal'
              >
                {APP_MODES.find(mode => mode.id === appMode)?.label ?? appMode}
                <Box
                  as={ChevronDown}
                  width='12px'
                  height='12px'
                  color='whiteAlpha.500'
                />
              </Button>
            </MenuTrigger>
            <MenuContent minWidth='110px'>
              {APP_MODES.map(mode => (
                <MenuItem
                  key={mode.id}
                  value={mode.id}
                  onClick={() => onAppModeChange(mode.id)}
                  justifyContent='space-between'
                >
                  <Box fontSize='11px'>{mode.label}</Box>
                  {mode.id === appMode && (
                    <Box as={Check} width='12px' height='12px' />
                  )}
                </MenuItem>
              ))}
            </MenuContent>
          </MenuRoot>
        </SettingRow>
      </SettingCard>

      <SettingCard
        title='Window'
        description='Drag the window edges to resize it. kftray keeps its size and position.'
      >
        <Flex justify='flex-end'>
          <Button
            {...compactActionButtonProps}
            onClick={resetPosition}
            loading={isResettingPosition}
            loadingText='...'
          >
            <Box as={LocateFixed} width='8px' height='8px' mr={0.5} />
            Reset Position
          </Button>
        </Flex>
      </SettingCard>
    </>
  )
}
