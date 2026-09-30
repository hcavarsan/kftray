import { Main } from '@/components/Main'
import { GitSyncProvider } from '@/contexts/GitSyncContext'

export function App() {
  return (
    <GitSyncProvider>
      <Main />
    </GitSyncProvider>
  )
}
