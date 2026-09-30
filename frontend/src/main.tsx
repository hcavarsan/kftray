import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'

import App from './App'
import { Provider } from './components/ui/provider'

import './index.css'

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
