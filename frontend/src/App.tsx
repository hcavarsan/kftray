import React from 'react'

import Main from '@/components/Main'
import { GitSyncProvider } from '@/contexts/GitSyncContext'

const App: React.FC = () => {
  return (
    <GitSyncProvider>
      <Main />
    </GitSyncProvider>
  )
}

export default App
