import { Heading, Image } from '@chakra-ui/react'

import logo from './../logo.png'

const Header = () => {
  return (
    <Heading
      as='h1'
      size='lg'
      color='white'
      mb={5}
	  mt={10}
      marginTop={-2}
      background='transparent'
    >
      <Image boxSize='96px' src={logo} />
    </Heading>
  )
}

export { Header }
