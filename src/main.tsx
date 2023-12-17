import React from 'react'
import { createRoot } from 'react-dom/client'

import { ChakraProvider } from '@chakra-ui/react'

import theme from './assets/theme' // Make sure this theme import path is correct
import App from './App'

import './assets/style.css'

const root = createRoot(document.getElementById('root'))


root.render(
  <React.StrictMode>
    <ChakraProvider theme={theme}>
      <App />
    </ChakraProvider>
  </React.StrictMode>,
)
