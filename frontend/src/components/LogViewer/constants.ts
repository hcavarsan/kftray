import type { LogLevel } from './types'

export const LEVEL_COLORS: Record<
  LogLevel,
  { bg: string; text: string; border: string }
> = {
  ERROR: {
    bg: 'rgba(229, 62, 62, 0.15)',
    text: 'rgba(252, 129, 129, 1)',
    border: 'rgba(229, 62, 62, 0.3)',
  },
  WARN: {
    bg: 'rgba(161, 98, 7, 0.15)',
    text: 'rgba(251, 191, 36, 1)',
    border: 'rgba(161, 98, 7, 0.3)',
  },
  INFO: {
    bg: 'app.accentSubtle',
    text: 'rgba(147, 197, 253, 1)',
    border: 'app.accentMuted',
  },
  DEBUG: {
    bg: 'rgba(139, 92, 246, 0.15)',
    text: 'rgba(196, 181, 253, 1)',
    border: 'rgba(139, 92, 246, 0.3)',
  },
  TRACE: {
    bg: 'rgba(100, 116, 139, 0.15)',
    text: 'rgba(148, 163, 184, 1)',
    border: 'rgba(100, 116, 139, 0.3)',
  },
}

export const ALL_LEVELS: LogLevel[] = [
  'ERROR',
  'WARN',
  'INFO',
  'DEBUG',
  'TRACE',
]

export const ROW_HEIGHT_COLLAPSED = 36

export const DEFAULT_LOG_LINES = 1000

export const AUTO_REFRESH_INTERVAL = 2000
