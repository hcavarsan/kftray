import React from 'react'

import { ChakraProvider } from '@chakra-ui/react'

import { system } from '@/assets/theme'
import { ColorModeProvider } from '@/components/ui/color-mode'

import Main from './components/Main'
import { Provider } from './components/ui/provider'

const App: React.FC = () => {
  return (
    <ChakraProvider value={system}>
      <ColorModeProvider forcedTheme="dark">
        <Provider>
          <Main />
        </Provider>
      </ColorModeProvider>
    </ChakraProvider>
  )
}

export default App
