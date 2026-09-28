import type React from 'react'
import { useState } from 'react'
import { LocateFixed } from 'lucide-react'

import { Box, Flex, NativeSelect, Text } from '@chakra-ui/react'
import { invoke } from '@tauri-apps/api/core'

import { Button } from '@/components/ui/button'
import { toaster } from '@/components/ui/toaster'

interface WindowSettingsProps {
  isLoading: boolean
  appMode: string
  sizePreset: string
  onAppModeChange: (mode: string) => void
  onSizePresetChange: (preset: string) => void
}

interface SelectOption {
  id: string
  label: string
}

const APP_MODES: SelectOption[] = [
  { id: 'tray', label: 'Tray' },
  { id: 'window', label: 'Window' },
]

const SIZE_PRESETS: SelectOption[] = [
  { id: 'xs', label: 'Extra Small' },
  { id: 'small', label: 'Small' },
  { id: 'default', label: 'Default' },
  { id: 'medium', label: 'Medium' },
  { id: 'large', label: 'Large' },
  { id: 'xl', label: 'Extra Large' },
]

interface CompactSelectProps {
  value: string
  options: SelectOption[]
  disabled: boolean
  onChange: (value: string) => void
}

const CompactSelect: React.FC<CompactSelectProps> = ({
  value,
  options,
  disabled,
  onChange,
}) => (
  <NativeSelect.Root size='xs' width='110px' disabled={disabled}>
    <NativeSelect.Field
      value={value}
      onChange={e => onChange(e.currentTarget.value)}
      height='22px'
      bg='#111111'
      border='1px solid rgba(255, 255, 255, 0.08)'
      _hover={{ borderColor: 'rgba(255, 255, 255, 0.15)' }}
      _focus={{ borderColor: 'blue.400', boxShadow: 'none' }}
      color='white'
      fontSize='xs'
    >
      {options.map(option => (
        <option key={option.id} value={option.id}>
          {option.label}
        </option>
      ))}
    </NativeSelect.Field>
    <NativeSelect.Indicator color='whiteAlpha.500' />
  </NativeSelect.Root>
)

const WindowSettings: React.FC<WindowSettingsProps> = ({
  isLoading,
  appMode,
  sizePreset,
  onAppModeChange,
  onSizePresetChange,
}) => {
  const [isResettingPosition, setIsResettingPosition] = useState(false)

  const resetPosition = async () => {
    setIsResettingPosition(true)
    try {
      await invoke('reset_window_position_cmd')
    } catch (error) {
      console.error('Error resetting window position:', error)
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
      <Box
        bg='#161616'
        p={2}
        borderRadius='md'
        border='1px solid rgba(255, 255, 255, 0.08)'
        display='flex'
        flexDirection='column'
        height='100%'
      >
        <Text fontSize='sm' fontWeight='500' color='white' mb={1}>
          App Mode
        </Text>
        <Text fontSize='xs' color='whiteAlpha.600' lineHeight='1.3' flex='1'>
          Tray opens from the tray icon and hides when it loses focus, falling
          back to Window when no tray is available. Window runs as a regular
          app.
        </Text>
        <Box borderTop='1px solid rgba(255, 255, 255, 0.06)' mt={3} pt={3}>
          <Flex align='center' justify='flex-end' gap={2}>
            <Text fontSize='xs' color='whiteAlpha.500'>
              Mode:
            </Text>
            <CompactSelect
              value={appMode}
              options={APP_MODES}
              disabled={isLoading}
              onChange={onAppModeChange}
            />
          </Flex>
        </Box>
      </Box>

      <Box
        bg='#161616'
        p={2}
        borderRadius='md'
        border='1px solid rgba(255, 255, 255, 0.08)'
        display='flex'
        flexDirection='column'
        height='100%'
      >
        <Text fontSize='sm' fontWeight='500' color='white' mb={1}>
          Window
        </Text>
        <Text fontSize='xs' color='whiteAlpha.600' lineHeight='1.3' flex='1'>
          Size of the main window and its saved position.
        </Text>
        <Box borderTop='1px solid rgba(255, 255, 255, 0.06)' mt={3} pt={3}>
          <Flex align='center' justify='flex-end' gap={2} mb={1}>
            <Text fontSize='xs' color='whiteAlpha.500'>
              Size:
            </Text>
            <CompactSelect
              value={sizePreset}
              options={SIZE_PRESETS}
              disabled={isLoading}
              onChange={onSizePresetChange}
            />
          </Flex>
          <Flex align='center' justify='flex-end'>
            <Button
              size='2xs'
              variant='outline'
              onClick={resetPosition}
              loading={isResettingPosition}
              loadingText='...'
              disabled={isLoading}
              height='18px'
              fontSize='10px'
              color='whiteAlpha.600'
              borderColor='rgba(255, 255, 255, 0.1)'
              _hover={{
                borderColor: 'rgba(255, 255, 255, 0.2)',
                bg: 'whiteAlpha.50',
              }}
              px={1.5}
            >
              <Box as={LocateFixed} width='8px' height='8px' mr={0.5} />
              Reset Position
            </Button>
          </Flex>
        </Box>
      </Box>
    </>
  )
}

export default WindowSettings
