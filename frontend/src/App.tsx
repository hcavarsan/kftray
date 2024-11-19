import React from 'react'

import Main from './components/Main'
import { Provider } from './components/ui/provider'

const App: React.FC = () => {
  return (
    <Provider>
      <Main />
    </Provider>
  )
}

export default App
