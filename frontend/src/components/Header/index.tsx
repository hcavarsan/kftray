import React from 'react'

import { DragHandleIcon, SearchIcon } from '@chakra-ui/icons'
import {
  Flex,
  IconButton,
  Image,
  Input,
  InputGroup,
  InputLeftElement,
  Tooltip,
} from '@chakra-ui/react'
import { app, window as tauriWindow } from '@tauri-apps/api'

import logo from '../../assets/logo.png'
import { HeaderProps } from '../../types'

const Header: React.FC<HeaderProps> = ({ search, setSearch }) => {
  const [version, setVersion] = React.useState('')

  React.useEffect(() => {
    app.getVersion().then(setVersion).catch(console.error)
  }, [])

  const handleSearchChange = (event: React.ChangeEvent<HTMLInputElement>) => {
    setSearch(event.target.value)
  }

  const handleDragStart = () => {
    tauriWindow.getCurrent().startDragging().catch(console.error)
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
      <Flex alignItems='center'>
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
      <Flex alignItems='center'>
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
        <Tooltip
          label='Move Window Position'
          aria-label='position'
          fontSize='xs'
          lineHeight='tight'
          placement='top-end'
        >
          <IconButton
            height='25px'
            aria-label='Drag window'
            icon={<DragHandleIcon />}
            size='xs'
            onMouseDown={handleDragStart}
            variant='ghost'
            mt='1.5'
            ml='1'
            colorScheme='gray'
          />
        </Tooltip>
      </Flex>
    </Flex>
  )
}

export default Header
