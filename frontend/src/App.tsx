import React from 'react'

import Main from './components/Main'
import { Provider } from './components/ui/provider'
import { Toaster } from './components/ui/toaster'

const App: React.FC = () => {
  return (
    <Provider>
      <Main />
      <Toaster />
    </Provider>
  )
}

export default App
