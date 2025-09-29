'use client'

import type { ThemeProviderProps } from 'next-themes'
import { ThemeProvider } from 'next-themes'

export interface ColorModeProviderProps extends ThemeProviderProps {}

export function ColorModeProvider(props: ColorModeProviderProps) {
  return (
    <ThemeProvider
      attribute='class'
      disableTransitionOnChange
      forcedTheme='dark'
      {...props}
    />
  )
}
