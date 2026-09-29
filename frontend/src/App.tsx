import Main from '@/components/Main'
import { GitSyncProvider } from '@/contexts/GitSyncContext'

function App() {
  return (
    <GitSyncProvider>
      <Main />
    </GitSyncProvider>
  )
}

export default App
