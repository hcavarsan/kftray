import React from 'react'
import ReactDOM from 'react-dom/client'
import { QueryClient, QueryClientProvider } from 'react-query'
import { attachConsole } from 'tauri-plugin-log-api'

import { ChakraProvider } from '@chakra-ui/react'

import theme from './assets/theme'
import App from './App'

import './assets/style.css'

if (import.meta.env.DEV) {
  attachConsole()
}

const queryClient = new QueryClient()

const rootElement = document.getElementById('root')

if (!rootElement) {
  throw new Error('Failed to find the root element')
}

ReactDOM.createRoot(rootElement).render(
  <React.StrictMode>
    <QueryClientProvider client={queryClient}>
      {' '}
      <ChakraProvider theme={theme}>
        <App />
      </ChakraProvider>
    </QueryClientProvider>
  </React.StrictMode>,
)
