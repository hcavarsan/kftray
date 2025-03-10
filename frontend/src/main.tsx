import React from 'react'
import ReactDOM from 'react-dom/client'

import { QueryClient, QueryClientProvider } from '@tanstack/react-query'

import { Provider } from './components/ui/provider'
import { Toaster } from './components/ui/toaster'
import App from './App'

const queryClient = new QueryClient()

const rootElement = document.getElementById('root')

if (!rootElement) {
  throw new Error('Failed to find the root element')
}

ReactDOM.createRoot(rootElement).render(
  <React.StrictMode>
    <QueryClientProvider client={queryClient}>
      <Provider forcedTheme='dark'>
        <App />
        <Toaster />
      </Provider>
    </QueryClientProvider>
  </React.StrictMode>,
)
