import React from 'react'

import { ChakraProvider } from '@chakra-ui/react'

import theme from './assets/theme'
import Main from './components/Main'

import './assets/style.css'

const App: React.FC = () => {
  return (
    <ChakraProvider theme={theme}>
      <Main />
    </ChakraProvider>
  )
}

export default App
