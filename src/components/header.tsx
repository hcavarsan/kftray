import { Heading, Image } from '@chakra-ui/react'
import logo from './../logo.png'

const Header = () => {
  return (
    <Heading
      as='h1'
      size='lg'
      color='white'
      mb={0}
      marginTop={-2}
      background='transparent'
    >
      <Image boxSize='80px' src={logo} />
    </Heading>
  )
}

export { Header }
