import React, { useEffect, useState } from 'react'

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
import { app, window as tauriWindow } from '@tauri-apps/api'

import logo from '../../assets/logo.png'
import { HeaderProps } from '../../types'

document.addEventListener('mousedown', async e => {
  const drag = '.drag-handle'
  const target = e.target as HTMLElement

  if (target.closest(drag)) {
    await tauriWindow.appWindow.startDragging()
    this.dispatchEvent(new PointerEvent('pointerup'))
    e.preventDefault()
  }
})

const Header: React.FC<HeaderProps> = ({ search, setSearch }) => {
  const [version, setVersion] = useState('')
  const [isDragging, setIsDragging] = useState(false)
  const [tooltipOpen, setTooltipOpen] = useState(false)

  useEffect(() => {
    app.getVersion().then(setVersion).catch(console.error)
  }, [])

  const handleSearchChange = (event: React.ChangeEvent<HTMLInputElement>) => {
    setSearch(event.target.value)
  }

  const handleMouseDown = () => {
    setIsDragging(true)
    setTooltipOpen(false)
  }

  const handleMouseUp = () => {
    setIsDragging(false)
  }

  const handleMouseEnter = () => {
    if (!isDragging) {
      setTooltipOpen(true)
    }
  }

  const handleMouseLeave = () => {
    setTooltipOpen(false)
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
          className='drag-handle'
          onMouseDown={handleMouseDown}
          onMouseUp={handleMouseUp}
          onMouseEnter={handleMouseEnter}
          onMouseLeave={handleMouseLeave}
          justifyContent='flex-start'
          mt='-0.5'
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
