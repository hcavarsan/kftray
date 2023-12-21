import React from 'react'

import { ChakraProvider } from '@chakra-ui/react'

import theme from './assets/theme'
import KFTray from './components/KFtray'

import './assets/style.css'

const App: React.FC = () => {
  return (
    <ChakraProvider theme={theme}>
      <KFTray />
    </ChakraProvider>
  )
}

export default App
