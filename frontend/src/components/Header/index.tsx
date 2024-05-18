// Header.tsx
import React from 'react'

import { SearchIcon } from '@chakra-ui/icons'
import {
  Image,
  Input,
  InputGroup,
  InputLeftElement,
  Tooltip,
} from '@chakra-ui/react'
import { app } from '@tauri-apps/api'

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

  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'space-between',
        backgroundColor: 'gray.800',
        borderRadius: 'lg',
        width: '100%',
        borderColor: 'gray.200',
        padding: '2px',
      }}
    >
      <Tooltip
        label={`Kftray v${version}`}
        aria-label='Kftray version'
        fontSize='xs'
        lineHeight='tight'
      >
        <Image src={logo} alt='Kftray Logo' boxSize='32px' ml={3} mt={0.5} />
      </Tooltip>
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
    </div>
  )
}

export default Header
