import React from 'react'

import { Heading, Image } from '@chakra-ui/react'

import logo from '../../assets/logo.png'

const Header: React.FC = () => {
  return (
    <Heading as='h1' size='sm' color='white' background='transparent'>
      <Image boxSize='72px' src={logo} />
    </Heading>
  )
}

export default Header
