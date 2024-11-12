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

  const handleSearchChange = (event: React.ChangeEvent<HTMLInputElement>) => {
    setSearch(event.target.value)
  }

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
      display="flex"
      alignItems="center"
      justifyContent="space-between"
      bg="#161616"
      borderRadius="lg"
      width="100%"

      px={2}
      py={2}
      border="1px solid rgba(255, 255, 255, 0.08)"
    >
      {/* Left Section - Logo and Drag Handle */}
      <Box
        display="flex"
        alignItems="center"
        gap={2}
      >
        <Box
          ref={dragHandleRef}
          className="drag-handle"
          onMouseEnter={() => setTooltipOpen(true)}
          onMouseLeave={() => setTooltipOpen(false)}
          cursor="move"
          p={1}
          _hover={{ color: 'whiteAlpha.700' }}
        >
          <Tooltip content="Move Window Position" open={tooltipOpen}>
            <Box
              as={GripVertical}
              width="16px"
              height="16px"
              color="whiteAlpha.500"
              data-drag
            />
          </Tooltip>
        </Box>

        <Tooltip content={`Kftray v${version}`}>
          <Image
            src={logo}
            alt="Kftray Logo"
            width="24px"
            height="24px"
            objectFit="contain"
            filter="brightness(0.9)"
            _hover={{ filter: 'brightness(1)' }}
            transition="filter 0.2s"
          />
        </Tooltip>
      </Box>

      {/* Right Section - Search and Controls */}
      <Box
        display="flex"
        alignItems="center"
        justifyContent="flex-end"

        gap={4}
      >
        {/* Search Input */}
        <Box
          position="relative"

          maxWidth="200px"
        >
          <Box
            as={Search}
            position="absolute"
            left={20}
            top="50%"
            transform="translateY(-50%)"
            width="14px"
            height="14px"
            color="whiteAlpha.500"
          />
          <Input
            value={search}
            onChange={handleSearchChange}
            placeholder="Search..."
            size="sm"
            pl={1}
            bg="#1A1A1A"
            border="1px solid rgba(255, 255, 255, 0.08)"
            _hover={{
              borderColor: 'rgba(255, 255, 255, 0.15)'
            }}
            _focus={{
              borderColor: 'blue.400',
              boxShadow: 'none'
            }}
            height="26px"
            fontSize="13px"
            width="80%"

            maxWidth="80%"
            color="whiteAlpha.900"
            _placeholder={{
              color: 'whiteAlpha.400'
            }}
          />
        </Box>

        {/* Window Controls */}
        <Box
          display="flex"
          alignItems="center"
          gap={1}
        >
          <Tooltip content={isPinned ? 'Unpin Window' : 'Pin Window'}>
            <Button
              variant="ghost"
              size="sm"
              onClick={togglePinWindow}
              height="32px"
              width="32px"
              minWidth="32px"
              p={0}
              _hover={{ bg: 'whiteAlpha.100' }}
              _active={{ bg: 'whiteAlpha.200' }}
            >
              <Box
                as={isPinned ? TiPin : TiPinOutline}
                width="16px"
                height="16px"
                color="whiteAlpha.700"
              />
            </Button>
          </Tooltip>

          <Tooltip content="Close Window">
            <Button
              variant="ghost"
              size="sm"
              onClick={handleStopPortForwardsAndExit}
              height="32px"
              width="32px"
              minWidth="32px"
              p={0}
              _hover={{ bg: 'whiteAlpha.100' }}
              _active={{ bg: 'whiteAlpha.200' }}
            >
              <Box
                as={MdClose}
                width="16px"
                height="16px"
                color="whiteAlpha.700"
              />
            </Button>
          </Tooltip>
        </Box>
      </Box>

    </Box>
  )
}

export default Header
