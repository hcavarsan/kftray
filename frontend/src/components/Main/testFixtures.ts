import { vi } from 'vitest'

import type { Config } from '@/types'

export const makeConfig = (id: number, isRunning = false): Config => ({
  id,
  workload_type: 'service',
  service: `service-${id}`,
  namespace: 'default',
  protocol: 'tcp',
  local_port: 8000 + id,
  remote_port: 80,
  is_running: isRunning,
})

export const flushPromises = () => vi.advanceTimersByTimeAsync(0)
