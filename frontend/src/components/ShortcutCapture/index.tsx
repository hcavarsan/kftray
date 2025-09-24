import React, { useCallback, useEffect, useRef, useState } from 'react'
import { Keyboard } from 'lucide-react'

import { Box, Flex, Text } from '@chakra-ui/react'

import { Button } from '@/components/ui/button'

/* eslint-disable complexity */

interface ShortcutCaptureProps {
  value: string
  onChange: (shortcut: string) => void
  disabled?: boolean
}

const ShortcutCapture: React.FC<ShortcutCaptureProps> = ({
  value,
  onChange,
  disabled = false,
}) => {
  const [isCapturing, setIsCapturing] = useState(false)
  const [capturedKeys, setCapturedKeys] = useState<string>('')
  const captureRef = useRef<HTMLDivElement>(null)

  const formatKey = useCallback((key: string): string => {
    const keyMap: Record<string, string> = {
      Control: 'Ctrl',
      Meta: 'Cmd',
      ArrowUp: 'Up',
      ArrowDown: 'Down',
      ArrowLeft: 'Left',
      ArrowRight: 'Right',
      ' ': 'Space',
    }

    return keyMap[key] || key
  }, [])

  const handleKeyDown = useCallback(
    (event: KeyboardEvent) => {
      if (!isCapturing) {
        return
      }

      event.preventDefault()
      event.stopPropagation()

      const modifiers: string[] = []
      const keys: string[] = []

      if (event.ctrlKey || event.metaKey) {
        modifiers.push(event.ctrlKey ? 'Ctrl' : 'Cmd')
      }
      if (event.altKey) {
        modifiers.push('Alt')
      }
      if (event.shiftKey) {
        modifiers.push('Shift')
      }

      if (
        !['Control', 'Alt', 'Shift', 'Meta'].includes(event.key) &&
        event.key.length > 0
      ) {
        keys.push(formatKey(event.key))

        const shortcutString = [...modifiers, ...keys].join('+')

        setCapturedKeys(shortcutString)

        setTimeout(() => {
          const backendFormat = shortcutString.toLowerCase()

          onChange(backendFormat)
          setIsCapturing(false)
          setCapturedKeys('')
        }, 500)
      } else if (modifiers.length > 0) {
        setCapturedKeys(modifiers.join('+') + '+')
      }
    },
    [isCapturing, formatKey, onChange],
  )

  const handleKeyUp = useCallback(
    (event: KeyboardEvent) => {
      if (!isCapturing) {
        return
      }

      event.preventDefault()
      event.stopPropagation()

      if (['Control', 'Alt', 'Shift', 'Meta'].includes(event.key)) {
        const remaining: string[] = []

        if (event.ctrlKey || event.metaKey) {
          remaining.push(event.ctrlKey ? 'Ctrl' : 'Cmd')
        }
        if (event.altKey) {
          remaining.push('Alt')
        }
        if (event.shiftKey) {
          remaining.push('Shift')
        }

        setCapturedKeys(remaining.length > 0 ? remaining.join('+') + '+' : '')
      }
    },
    [isCapturing],
  )

  const startCapture = useCallback(() => {
    if (disabled) {
      return
    }
    setIsCapturing(true)
    setCapturedKeys('')
    captureRef.current?.focus()
  }, [disabled])

  const stopCapture = useCallback(() => {
    setIsCapturing(false)
    setCapturedKeys('')
  }, [])

  const handleBlur = useCallback(() => {
    if (isCapturing) {
      stopCapture()
    }
  }, [isCapturing, stopCapture])

  useEffect(() => {
    if (isCapturing) {
      document.addEventListener('keydown', handleKeyDown, true)
      document.addEventListener('keyup', handleKeyUp, true)

      return () => {
        document.removeEventListener('keydown', handleKeyDown, true)
        document.removeEventListener('keyup', handleKeyUp, true)
      }
    }
  }, [isCapturing, handleKeyDown, handleKeyUp])

  const displayValue = isCapturing
    ? capturedKeys || 'Press any key combination...'
    : value || 'Click to set shortcut'

  return (
    <Box position='relative'>
      <Flex
        ref={captureRef}
        align='center'
        justify='space-between'
        p={2}
        bg={isCapturing ? '#0a0a0a' : '#161616'}
        border={`1px solid ${
          isCapturing ? 'rgba(59, 130, 246, 0.5)' : 'rgba(255, 255, 255, 0.08)'
        }`}
        borderRadius='md'
        cursor={disabled ? 'not-allowed' : 'pointer'}
        _hover={
          !disabled && !isCapturing
            ? { borderColor: 'rgba(255, 255, 255, 0.15)' }
            : {}
        }
        _focus={
          !disabled
            ? {
                borderColor: 'blue.400',
                boxShadow: '0 0 0 1px rgba(59, 130, 246, 0.3)',
              }
            : {}
        }
        onClick={startCapture}
        onBlur={handleBlur}
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
          zIndex={10}
        >
          Hold modifiers (Ctrl/Cmd/Alt/Shift) + press any key
        </Text>
      )}
    </Box>
  )
}

export default ShortcutCapture
