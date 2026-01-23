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
    bg: 'rgba(59, 130, 246, 0.15)',
    text: 'rgba(147, 197, 253, 1)',
    border: 'rgba(59, 130, 246, 0.3)',
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

export const ROW_HEIGHT_EXPANDED_DEFAULT = 180

export const DEFAULT_LOG_LINES = 1000

export const AUTO_REFRESH_INTERVAL = 2000

export const COLORS = {
  bgPrimary: '#111111',
  bgSecondary: '#161616',
  bgTertiary: '#141414',
  bgDeep: '#0a0a0a',
  bgInput: '#1A1A1A',

  borderDefault: 'rgba(255, 255, 255, 0.08)',
  borderSubtle: 'rgba(255, 255, 255, 0.05)',
  borderHover: 'rgba(255, 255, 255, 0.15)',

  textPrimary: 'white',
  textSecondary: 'rgba(255, 255, 255, 0.7)',
  textMuted: 'rgba(255, 255, 255, 0.5)',
  textDisabled: 'rgba(255, 255, 255, 0.4)',

  accentBlue: 'rgb(59, 130, 246)',
  accentBlueMuted: 'rgba(59, 130, 246, 0.8)',
  accentCyan: 'rgba(34, 211, 238, 0.9)',
}
