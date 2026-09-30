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

function keyLabel(event: KeyboardEvent): string {
  const match = /^(?:Key([A-Z])|Digit(\d))$/.exec(event.code)

  if (match) {
    return (match[1] ?? match[2]).toLowerCase()
  }

  return KEY_LABELS[event.key] ?? event.key
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

export function ShortcutCapture({
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
      if (event.key === 'Escape') {
        setIsCapturing(false)
        setCapturedKeys('')
        return
      }

      const modifiers = activeModifiers(event)

      if (!MODIFIER_KEYS.includes(event.key) && event.key.length > 0) {
        commit([...modifiers, keyLabel(event)].join('+'))
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

    window.addEventListener('keydown', handleKeyDown, true)
    window.addEventListener('keyup', handleKeyUp, true)

    return () => {
      window.removeEventListener('keydown', handleKeyDown, true)
      window.removeEventListener('keyup', handleKeyUp, true)
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
        bg={isCapturing ? 'bg.deep' : 'bg.surface'}
        border='1px solid'
        borderColor={isCapturing ? 'accent.emphasized' : 'border'}
        borderRadius='md'
        cursor={disabled ? 'not-allowed' : 'pointer'}
        _hover={
          !disabled && !isCapturing ? { borderColor: 'border.emphasized' } : {}
        }
        _focus={
          !disabled
            ? {
                borderColor: 'accent.focusRing',
                boxShadow: '0 0 0 1px var(--chakra-colors-accent-muted)',
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
            color={isCapturing ? 'accent.fg' : 'fg.subtle'}
          />
          <Text
            fontSize='xs'
            color={isCapturing ? 'accent.fg' : value ? 'fg' : 'fg.subtle'}
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
            color='fg.subtle'
            _hover={{ color: 'fg', bg: 'bg.hover' }}
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
          color='accent.fg'
          bg='bg.scrim'
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
