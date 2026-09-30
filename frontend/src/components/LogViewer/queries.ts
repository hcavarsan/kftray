import { queryOptions } from '@tanstack/react-query'
import { invoke } from '@tauri-apps/api/core'

import type { LogFileInfo } from './types'

export const logFilesQuery = queryOptions({
  queryKey: ['log-files'],
  queryFn: () => invoke<LogFileInfo[]>('list_log_files'),
})
