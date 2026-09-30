import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'

import { Provider } from './components/ui/provider'
import { LogViewerPage } from './pages/LogViewerPage'

import './index.css'

const rootElement = document.getElementById('root')

if (!rootElement) {
  throw new Error('Failed to find the root element')
}

createRoot(rootElement).render(
  <StrictMode>
    <Provider>
      <LogViewerPage />
    </Provider>
  </StrictMode>,
)
