import type { Dispatch, SetStateAction } from 'react'
import { useEffect, useRef, useState } from 'react'
import { GripVertical, Minus, Pin, PinOff, Search, X } from 'lucide-react'

import { Box, Image, Input } from '@chakra-ui/react'
import { useMutation } from '@tanstack/react-query'
import { app } from '@tauri-apps/api'
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow'

import logo from '@/assets/logo.webp'
import { Button } from '@/components/ui/button'
import { Tooltip } from '@/components/ui/tooltip'
import { useTauriEvent } from '@/hooks/useTauriEvent'
import { invoke } from '@/lib/tauri'

const appWindow = getCurrentWebviewWindow()

type TrayMode = 'tray' | 'window'

interface HeaderProps {
  search: string
  setSearch: Dispatch<SetStateAction<string>>
}

export function Header({ search, setSearch }: HeaderProps) {
  const [version, setVersion] = useState('')
  const [tooltipOpen, setTooltipOpen] = useState(false)
  const [isPinned, setIsPinned] = useState(false)
  const [trayMode, setTrayMode] = useState<TrayMode>('tray')
  const dragHandleRef = useRef<HTMLDivElement | null>(null)

  useEffect(() => {
    app
      .getVersion()
      .then(setVersion)
      .catch(() => undefined)
    invoke<TrayMode>('get_tray_mode_cmd')
      .then(setTrayMode)
      .catch(() => undefined)
  }, [])

  useTauriEvent<boolean>('pin-state-changed', event => {
    setIsPinned(event.payload)
  })

  useTauriEvent<TrayMode>('tray-mode-changed', event => {
    setTrayMode(event.payload)
  })

  useEffect(() => {
    if (!dragHandleRef.current) {
      return
    }

    const handleMouseMove = async (e: MouseEvent) => {
      if (e.buttons === 1) {
        e.preventDefault()
        await appWindow.startDragging()
      }
    }

    const handleMouseDown = (_e: MouseEvent) => {
      setTooltipOpen(false)
      document.addEventListener('mousemove', handleMouseMove)
    }

    const handleMouseUp = () => {
      document.removeEventListener('mousemove', handleMouseMove)
    }

    const currentDragHandle = dragHandleRef.current

    currentDragHandle.addEventListener('mousedown', handleMouseDown)
    document.addEventListener('mouseup', handleMouseUp)

    return () => {
      currentDragHandle.removeEventListener('mousedown', handleMouseDown)
      document.removeEventListener('mouseup', handleMouseUp)
    }
  }, [])

  const exitMutation = useMutation({
    mutationFn: () => invoke('handle_exit_app'),
    meta: { errorToast: { title: 'Failed to exit' } },
  })

  const pinMutation = useMutation({
    mutationFn: async (next: boolean) => {
      await invoke('toggle_pin_state')
      if (next) {
        await appWindow.show()
        await appWindow.setFocus()
      }
    },
    onMutate: next => {
      setIsPinned(next)
    },
    onError: (_error, next) => {
      setIsPinned(!next)
    },
    meta: { errorToast: { title: 'Failed to toggle pin' } },
  })

  const hideMutation = useMutation({
    mutationFn: () => invoke('hide_main_window_cmd'),
    meta: { errorToast: { title: 'Failed to hide window' } },
  })

  return (
    <Box
      display='flex'
      alignItems='center'
      justifyContent='space-between'
      bg='bg.surface'
      borderRadius='lg'
      borderBottomRadius='none'
      width='100%'
      px={3}
      py={3}
      borderBottom='none'
      border='1px solid'
      borderColor='border'
      position='relative'
      zIndex={10}
    >
      <Box display='flex' alignItems='center' gap={3}>
        <Box display='flex' alignItems='center' gap={2}>
          <Box
            ref={dragHandleRef}
            className='drag-handle'
            onMouseEnter={() => setTooltipOpen(true)}
            onMouseLeave={() => setTooltipOpen(false)}
            cursor='move'
            _hover={{ color: 'fg.muted' }}
            mb={0.5}
          >
            <Tooltip content='Move Window Position' open={tooltipOpen}>
              <Box
                as={GripVertical}
                width='22px'
                height='22px'
                color='fg.subtle'
                data-drag
              />
            </Tooltip>
          </Box>

          <Tooltip content={`Kftray v${version}`}>
            <Image
              src={logo}
              alt='Kftray Logo'
              width='33px'
              height='33px'
              objectFit='contain'
              filter='brightness(0.9)'
              _hover={{ filter: 'brightness(1)' }}
              transition='filter 0.2s'
            />
          </Tooltip>
        </Box>

        <Box position='relative' width='200px' ml={5}>
          <Box
            as={Search}
            position='absolute'
            zIndex={1}
            left={2}
            top='50%'
            transform='translateY(-50%)'
            width='14px'
            height='14px'
            color='fg.subtle'
          />
          <Input
            value={search}
            onChange={e => setSearch(e.target.value)}
            placeholder='Search...'
            size='sm'
            pl={8}
            bg='bg.raised'
            border='1px solid'
            borderColor='border'
            _hover={{
              borderColor: 'border.emphasized',
            }}
            _focus={{
              borderColor: 'accent.focusRing',
              boxShadow: 'none',
            }}
            height='28px'
            fontSize='13px'
            width='100%'
            color='fg'
            _placeholder={{
              color: 'fg.faint',
            }}
          />
        </Box>
      </Box>

      <Box display='flex' alignItems='center' gap={1} ml={4} mr={-1}>
        <Tooltip content={isPinned ? 'Unpin Window' : 'Pin Window'}>
          <Button
            aria-label={isPinned ? 'Unpin Window' : 'Pin Window'}
            variant='ghost'
            size='sm'
            onClick={() => pinMutation.mutate(!isPinned)}
            height='28px'
            width='28px'
            minWidth='28px'
            p={0}
            _hover={{ bg: 'bg.hover' }}
            _active={{ bg: 'bg.active' }}
          >
            <Box
              as={isPinned ? Pin : PinOff}
              width='16px'
              height='16px'
              color='fg.muted'
            />
          </Button>
        </Tooltip>

        <Tooltip
          content={trayMode === 'window' ? 'Minimize Window' : 'Hide Window'}
        >
          <Button
            aria-label={
              trayMode === 'window' ? 'Minimize Window' : 'Hide Window'
            }
            variant='ghost'
            size='sm'
            onClick={() => hideMutation.mutate()}
            height='28px'
            width='28px'
            minWidth='28px'
            p={0}
            ml={-1.5}
            _hover={{ bg: 'bg.hover' }}
            _active={{ bg: 'bg.active' }}
          >
            <Box as={Minus} width='15px' height='15px' color='fg.muted' />
          </Button>
        </Tooltip>

        <Tooltip content='Quit kftray'>
          <Button
            aria-label='Quit kftray'
            variant='ghost'
            size='sm'
            onClick={() => exitMutation.mutate()}
            height='28px'
            width='28px'
            minWidth='28px'
            p={0}
            ml={-1.5}
            _hover={{ bg: 'bg.hover' }}
            _active={{ bg: 'bg.active' }}
          >
            <Box as={X} width='15px' height='15px' color='fg.muted' />
          </Button>
        </Tooltip>
      </Box>
    </Box>
  )
}
