import type React from 'react'
import { useState } from 'react'
import { Check, ChevronDown, LocateFixed } from 'lucide-react'

import { Box, Flex, Text } from '@chakra-ui/react'
import { invoke } from '@tauri-apps/api/core'

import { Button } from '@/components/ui/button'
import {
  MenuContent,
  MenuItem,
  MenuRoot,
  MenuTrigger,
} from '@/components/ui/menu'
import { toaster } from '@/components/ui/toaster'

interface WindowSettingsProps {
  isLoading: boolean
  appMode: string
  onAppModeChange: (mode: string) => void
}

interface SelectOption {
  id: string
  label: string
}

const APP_MODES: SelectOption[] = [
  { id: 'tray', label: 'Tray' },
  { id: 'window', label: 'Window' },
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
  <MenuRoot positioning={{ sameWidth: true }}>
    <MenuTrigger asChild disabled={disabled}>
      <Button
        size='xs'
        width='110px'
        height='22px'
        px={2}
        justifyContent='space-between'
        bg='#111111'
        border='1px solid rgba(255, 255, 255, 0.08)'
        _hover={{ borderColor: 'rgba(255, 255, 255, 0.15)' }}
        _expanded={{ borderColor: 'blue.400' }}
        color='white'
        fontSize='xs'
        fontWeight='normal'
      >
        {options.find(option => option.id === value)?.label ?? value}
        <Box
          as={ChevronDown}
          width='12px'
          height='12px'
          color='whiteAlpha.500'
        />
      </Button>
    </MenuTrigger>
    <MenuContent portalled={false} minWidth='110px'>
      {options.map(option => (
        <MenuItem
          key={option.id}
          value={option.id}
          onClick={() => onChange(option.id)}
          justifyContent='space-between'
        >
          <Box fontSize='11px'>{option.label}</Box>
          {option.id === value && <Box as={Check} width='12px' height='12px' />}
        </MenuItem>
      ))}
    </MenuContent>
  </MenuRoot>
)

const WindowSettings: React.FC<WindowSettingsProps> = ({
  isLoading,
  appMode,
  onAppModeChange,
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
          Drag the window edges to resize it. kftray keeps its size and
          position.
        </Text>
        <Box borderTop='1px solid rgba(255, 255, 255, 0.06)' mt={3} pt={3}>
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
