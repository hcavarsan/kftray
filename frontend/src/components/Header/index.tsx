import React, { useEffect, useRef, useState } from 'react'
import { TiPin, TiPinOutline } from 'react-icons/ti'

import { DragHandleIcon, SearchIcon } from '@chakra-ui/icons'
import {
  Box,
  Flex,
  Icon,
  IconButton,
  Image,
  Input,
  InputGroup,
  InputLeftElement,
  Tooltip,
} from '@chakra-ui/react'
import { app } from '@tauri-apps/api'
import { invoke } from '@tauri-apps/api/tauri'
import { appWindow } from '@tauri-apps/api/window'

import logo from '../../assets/logo.webp'
import { HeaderProps } from '../../types'

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

  const handleMouseEnter = () => setTooltipOpen(true)
  const handleMouseLeave = () => setTooltipOpen(false)

  const togglePinWindow = async () => {
    setIsPinned(!isPinned)
    await invoke('toggle_pin_state')
    if (!isPinned) {
      await appWindow.show()
      await appWindow.setFocus()
    }
  }

  return (
    <Flex
      alignItems='center'
      justifyContent='space-between'
      backgroundColor='gray.800'
      borderRadius='lg'
      width='100%'
      borderColor='gray.200'
      padding='2px'
    >
      <Flex justifyContent='flex-start' alignItems='center'>
        <Box
          ref={dragHandleRef}
          className='drag-handle'
          onMouseEnter={handleMouseEnter}
          onMouseLeave={handleMouseLeave}
        >
          <Tooltip
            label='Move Window Position'
            aria-label='position'
            fontSize='xs'
            lineHeight='tight'
            closeOnMouseDown={true}
            isOpen={tooltipOpen}
          >
            <Icon
              as={DragHandleIcon}
              height='17px'
              width='17px'
              color='gray.500'
              data-drag
            />
          </Tooltip>
        </Box>
        <Tooltip
          label={`Kftray v${version}`}
          aria-label='Kftray version'
          fontSize='xs'
          lineHeight='tight'
          placement='top-end'
        >
          <Image src={logo} alt='Kftray Logo' boxSize='32px' ml={3} mt={0.5} />
        </Tooltip>
      </Flex>
      <Flex alignItems='center' justifyContent='flex-end'>
        <InputGroup size='xs' width='200px' mt='1'>
          <InputLeftElement pointerEvents='none'>
            <SearchIcon color='gray.300' />
          </InputLeftElement>
          <Input
            height='25px'
            type='text'
            placeholder='Search'
            value={search}
            onChange={handleSearchChange}
            borderRadius='md'
          />
        </InputGroup>
        <Box mt={-7} mr={-4}>
          <Tooltip
            label={isPinned ? 'Unpin Window' : 'Pin Window'}
            aria-label='Pin Window'
            fontSize='xs'
            lineHeight='tight'
            placement='top'
          >
            <IconButton
              aria-label='Pin Window'
              icon={isPinned ? <TiPin /> : <TiPinOutline />}
              onClick={togglePinWindow}
              size='sm'
              variant='ghost'
              color='gray.500'
              _hover={{ backgroundColor: 'transparent' }}
              _active={{ backgroundColor: 'transparent' }}
            />
          </Tooltip>
        </Box>
      </Flex>
    </Flex>
  )
}

export default Header
