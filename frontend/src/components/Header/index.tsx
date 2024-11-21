import React, { useEffect, useRef, useState } from 'react'
import { GripVertical, Search } from 'lucide-react'
import { MdClose } from 'react-icons/md'
import { TiPin, TiPinOutline } from 'react-icons/ti'

import { Box, Image, Input } from '@chakra-ui/react'
import { app } from '@tauri-apps/api'
import { invoke } from '@tauri-apps/api/tauri'
import { appWindow } from '@tauri-apps/api/window'

import logo from '@/assets/logo.webp'
import { Button } from '@/components/ui/button'
import { Tooltip } from '@/components/ui/tooltip'
import { HeaderProps } from '@/types'

const Header: React.FC<HeaderProps> = ({ search, setSearch }) => {
  const [version, setVersion] = useState('')
  const [tooltipOpen, setTooltipOpen] = useState(false)
  const [isPinned, setIsPinned] = useState(false)
  const dragHandleRef = useRef<HTMLDivElement | null>(null)

  useEffect(() => {
    app.getVersion().then(setVersion).catch(console.error)
  }, [])

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

  async function handleStopPortForwardsAndExit() {
    try {
      await invoke('handle_exit_app')
    } catch (error) {
      console.error('Error invoking handle_exit_app:', error)
    }
  }

  const togglePinWindow = async () => {
    setIsPinned(!isPinned)
    await invoke('toggle_pin_state')
    if (!isPinned) {
      await appWindow.show()
      await appWindow.setFocus()
    }
  }

  return (
    <Box
      display='flex'
      alignItems='center'
      justifyContent='space-between'
      bg='#161616'
      borderRadius='lg'
      borderBottomRadius='none'
      width='100%'
      px={3}
      py={3}
      borderBottom='none'
      border='1px solid rgba(255, 255, 255, 0.08)'
    >
      {/* Left Section */}
      <Box display='flex' alignItems='center' gap={3}>
        <Box display='flex' alignItems='center' gap={2}>
          <Box
            ref={dragHandleRef}
            className='drag-handle'
            onMouseEnter={() => setTooltipOpen(true)}
            onMouseLeave={() => setTooltipOpen(false)}
            cursor='move'
            _hover={{ color: 'whiteAlpha.700' }}
            mb={0.5}
          >
            <Tooltip content='Move Window Position' open={tooltipOpen}>
              <Box
                as={GripVertical}
                width='22px'
                height='22px'
                color='whiteAlpha.500'
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

        {/* Search Input */}
        <Box position='relative' width='200px' ml={12}>
          <Box
            as={Search}
            position='absolute'
            zIndex={100}
            left={2}
            top='50%'
            transform='translateY(-50%)'
            width='14px'
            height='14px'
            color='whiteAlpha.500'
          />
          <Input
            value={search}
            onChange={e => setSearch(e.target.value)}
            placeholder='Search...'
            size='sm'
            pl={8}
            bg='#1A1A1A'
            border='1px solid rgba(255, 255, 255, 0.08)'
            _hover={{
              borderColor: 'rgba(255, 255, 255, 0.15)',
            }}
            _focus={{
              borderColor: 'blue.400',
              boxShadow: 'none',
            }}
            height='28px'
            fontSize='13px'
            width='100%'
            color='whiteAlpha.900'
            _placeholder={{
              color: 'whiteAlpha.400',
            }}
          />
        </Box>
      </Box>

      {/* Right Section - Window Controls */}
      <Box display='flex' alignItems='center' gap={1}>
        <Tooltip content={isPinned ? 'Unpin Window' : 'Pin Window'}>
          <Button
            variant='ghost'
            size='sm'
            onClick={togglePinWindow}
            height='28px'
            width='28px'
            minWidth='28px'
            p={0}
            _hover={{ bg: 'whiteAlpha.100' }}
            _active={{ bg: 'whiteAlpha.200' }}
          >
            <Box
              as={isPinned ? TiPin : TiPinOutline}
              width='16px'
              height='16px'
              color='whiteAlpha.700'
            />
          </Button>
        </Tooltip>

        <Tooltip content='Close Window'>
          <Button
            variant='ghost'
            size='sm'
            onClick={handleStopPortForwardsAndExit}
            height='28px'
            width='28px'
            minWidth='28px'
            p={0}
            _hover={{ bg: 'whiteAlpha.100' }}
            _active={{ bg: 'whiteAlpha.200' }}
          >
            <Box
              as={MdClose}
              width='16px'
              height='16px'
              color='whiteAlpha.700'
            />
          </Button>
        </Tooltip>
      </Box>
    </Box>
  )
}

export default Header
