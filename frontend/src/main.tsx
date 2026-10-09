import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'

import { App } from './App'
import { Provider } from './components/ui/provider'
import { initCrashReporting } from './lib/telemetry'

import './index.css'

void initCrashReporting()

const rootElement = document.getElementById('root')

if (!rootElement) {
  throw new Error('Failed to find the root element')
}

createRoot(rootElement).render(
  <StrictMode>
    <Provider>
      <App />
    </Provider>
  </StrictMode>,
)
