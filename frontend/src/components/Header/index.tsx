import React, { useEffect, useRef, useState } from 'react'

import { DragHandleIcon, SearchIcon } from '@chakra-ui/icons'
import {
  Box,
  Flex,
  Icon,
  Image,
  Input,
  InputGroup,
  InputLeftElement,
  Tooltip,
} from '@chakra-ui/react'
import { app } from '@tauri-apps/api'
import { appWindow } from '@tauri-apps/api/window'

import logo from '../../assets/logo.png'
import { HeaderProps } from '../../types'

const Header: React.FC<HeaderProps> = ({ search, setSearch }) => {
  const [version, setVersion] = useState('')
  const [tooltipOpen, setTooltipOpen] = useState(false)
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
        <InputGroup size='xs' width='250px' mt='1'>
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
      </Flex>
    </Flex>
  )
}

export default Header
