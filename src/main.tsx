import React from 'react'
import ReactDOM from 'react-dom/client'
import { attachConsole } from 'tauri-plugin-log-api'

import { ChakraProvider } from '@chakra-ui/react'

import theme from './assets/theme' // Make sure this theme import path is correct
import App from './App'

import './assets/style.css'

if (import.meta.env.DEV) {
  attachConsole()
}
ReactDOM.createRoot(document.getElementById('root') as HTMLElement).render(
  <React.StrictMode>
    <ChakraProvider theme={theme}>
      <App />
    </ChakraProvider>
  </React.StrictMode>,
)
