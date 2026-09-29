import { useEffect, useEffectEvent, useRef, useState } from 'react'
import { Keyboard } from 'lucide-react'

import { Box, Flex, Text } from '@chakra-ui/react'

import { Button } from '@/components/ui/button'

const MODIFIER_KEYS = ['Control', 'Alt', 'Shift', 'Meta']

const KEY_LABELS: Record<string, string> = {
  Control: 'Ctrl',
  Meta: 'Cmd',
  ArrowUp: 'Up',
  ArrowDown: 'Down',
  ArrowLeft: 'Left',
  ArrowRight: 'Right',
  ' ': 'Space',
}

function activeModifiers(event: KeyboardEvent): string[] {
  const modifiers: string[] = []

  if (event.ctrlKey || event.metaKey) {
    modifiers.push(event.ctrlKey ? 'Ctrl' : 'Cmd')
  }
  if (event.altKey) {
    modifiers.push('Alt')
  }
  if (event.shiftKey) {
    modifiers.push('Shift')
  }

  return modifiers
}

interface ShortcutCaptureProps {
  value: string
  onChange: (shortcut: string) => void
  disabled?: boolean
}

export default function ShortcutCapture({
  value,
  onChange,
  disabled = false,
}: ShortcutCaptureProps) {
  const [isCapturing, setIsCapturing] = useState(false)
  const [capturedKeys, setCapturedKeys] = useState('')
  const captureRef = useRef<HTMLDivElement>(null)

  const stopCapture = () => {
    setIsCapturing(false)
    setCapturedKeys('')
  }

  const commit = useEffectEvent((shortcut: string) => {
    onChange(shortcut.toLowerCase())
    stopCapture()
  })

  useEffect(() => {
    if (!isCapturing) {
      return
    }

    const handleKeyDown = (event: KeyboardEvent) => {
      event.preventDefault()
      event.stopPropagation()
      if (event.repeat) {
        return
      }

      const modifiers = activeModifiers(event)

      if (!MODIFIER_KEYS.includes(event.key) && event.key.length > 0) {
        commit([...modifiers, KEY_LABELS[event.key] ?? event.key].join('+'))
      } else if (modifiers.length > 0) {
        setCapturedKeys(`${modifiers.join('+')}+`)
      }
    }

    const handleKeyUp = (event: KeyboardEvent) => {
      event.preventDefault()
      event.stopPropagation()
      if (MODIFIER_KEYS.includes(event.key)) {
        const remaining = activeModifiers(event)

        setCapturedKeys(remaining.length > 0 ? `${remaining.join('+')}+` : '')
      }
    }

    document.addEventListener('keydown', handleKeyDown, true)
    document.addEventListener('keyup', handleKeyUp, true)

    return () => {
      document.removeEventListener('keydown', handleKeyDown, true)
      document.removeEventListener('keyup', handleKeyUp, true)
    }
  }, [isCapturing])

  const startCapture = () => {
    if (disabled) {
      return
    }
    setIsCapturing(true)
    setCapturedKeys('')
    captureRef.current?.focus()
  }

  const displayValue = isCapturing
    ? capturedKeys || 'Press any key combination...'
    : value || 'Click to set shortcut'

  return (
    <Box position='relative'>
      <Flex
        ref={captureRef}
        role='button'
        aria-label='Set keyboard shortcut'
        aria-disabled={disabled}
        align='center'
        justify='space-between'
        p={2}
        bg={isCapturing ? 'app.deep' : 'app.panel'}
        border='1px solid'
        borderColor={isCapturing ? 'blue.500/50' : 'app.border'}
        borderRadius='md'
        cursor={disabled ? 'not-allowed' : 'pointer'}
        _hover={
          !disabled && !isCapturing ? { borderColor: 'app.borderStrong' } : {}
        }
        _focus={
          !disabled
            ? {
                borderColor: 'blue.400',
                boxShadow: '0 0 0 1px var(--chakra-colors-app-accent-muted)',
              }
            : {}
        }
        onClick={startCapture}
        onKeyDown={e => {
          if (!isCapturing && (e.key === 'Enter' || e.key === ' ')) {
            e.preventDefault()
            startCapture()
          }
        }}
        onBlur={stopCapture}
        tabIndex={disabled ? -1 : 0}
        opacity={disabled ? 0.5 : 1}
        minH='32px'
      >
        <Flex align='center' gap={2} flex={1}>
          <Box
            as={Keyboard}
            width='12px'
            height='12px'
            color={isCapturing ? 'blue.400' : 'whiteAlpha.600'}
          />
          <Text
            fontSize='xs'
            color={
              isCapturing ? 'blue.300' : value ? 'white' : 'whiteAlpha.500'
            }
            fontFamily={isCapturing || value ? 'mono' : 'inherit'}
            letterSpacing={isCapturing || value ? '0.5px' : 'normal'}
          >
            {displayValue}
          </Text>
        </Flex>

        {isCapturing && (
          <Button
            size='2xs'
            variant='ghost'
            onClick={e => {
              e.stopPropagation()
              stopCapture()
            }}
            color='whiteAlpha.600'
            _hover={{ color: 'white', bg: 'whiteAlpha.100' }}
            height='20px'
            px={1.5}
          >
            Cancel
          </Button>
        )}
      </Flex>

      {isCapturing && (
        <Text
          position='absolute'
          top='100%'
          left={0}
          mt={1}
          fontSize='2xs'
          color='blue.300'
          bg='rgba(0, 0, 0, 0.8)'
          px={2}
          py={1}
          borderRadius='sm'
          whiteSpace='nowrap'
          zIndex={5}
        >
          Hold modifiers (Ctrl/Cmd/Alt/Shift) + press any key
        </Text>
      )}
    </Box>
  )
}
