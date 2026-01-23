import React from 'react'
import ReactDOM from 'react-dom/client'

import { Provider } from './components/ui/provider'
import { Toaster } from './components/ui/toaster'
import LogViewerPage from './pages/LogViewerPage'

import './index.css'

const rootElement = document.getElementById('root')

if (!rootElement) {
  throw new Error('Failed to find the root element')
}

ReactDOM.createRoot(rootElement).render(
  <React.StrictMode>
    <Provider forcedTheme='dark'>
      <LogViewerPage />
      <Toaster />
    </Provider>
  </React.StrictMode>,
)
