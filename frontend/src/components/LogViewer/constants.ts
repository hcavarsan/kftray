import type { LogLevel } from './types'

export const LEVEL_COLORS: Record<
  LogLevel,
  { bg: string; text: string; border: string }
> = {
  ERROR: {
    bg: 'log.error.bg',
    text: 'log.error.text',
    border: 'log.error.border',
  },
  WARN: {
    bg: 'log.warn.bg',
    text: 'log.warn.text',
    border: 'log.warn.border',
  },
  INFO: {
    bg: 'app.accentSubtle',
    text: 'app.accentText',
    border: 'app.accentMuted',
  },
  DEBUG: {
    bg: 'log.debug.bg',
    text: 'log.debug.text',
    border: 'log.debug.border',
  },
  TRACE: {
    bg: 'log.trace.bg',
    text: 'log.trace.text',
    border: 'log.trace.border',
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
