import React from 'react'

import { Heading, Image } from '@chakra-ui/react'

import logo from '../../assets/logo.png'

const Header: React.FC = () => {
  return (
    <Heading
      as='h1'
      size='lg'
      color='white'
      mb={5}
      mt={1}
      background='transparent'
    >
      <Image boxSize='96px' src={logo} />
    </Heading>
  )
}

export default Header
