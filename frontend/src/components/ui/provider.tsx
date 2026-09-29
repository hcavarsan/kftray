import type { ReactNode } from 'react'
import { ThemeProvider } from 'next-themes'

import {
  ChakraProvider,
  createSystem,
  defaultConfig,
  defineConfig,
} from '@chakra-ui/react'
import { QueryClient, QueryClientProvider } from '@tanstack/react-query'

import { Toaster } from './toaster'

const color = (value: string) => ({ value })

const system = createSystem(
  defaultConfig,
  defineConfig({
    globalCss: {
      body: {
        backgroundColor: 'transparent',
        margin: 0,
      },
    },
    theme: {
      semanticTokens: {
        colors: {
          app: {
            bg: color('#111111'),
            deep: color('#0a0a0a'),
            sunken: color('#141414'),
            panel: color('#161616'),
            raised: color('#1A1A1A'),
            faint: color('rgba(255, 255, 255, 0.03)'),
            hover: color('rgba(255, 255, 255, 0.05)'),
            subtle: color('rgba(255, 255, 255, 0.06)'),
            border: color('rgba(255, 255, 255, 0.08)'),
            active: color('rgba(255, 255, 255, 0.1)'),
            borderStrong: color('rgba(255, 255, 255, 0.15)'),
            divider: color('rgba(255, 255, 255, 0.2)'),
            placeholder: color('rgba(255, 255, 255, 0.5)'),
            textSecondary: color('rgba(255, 255, 255, 0.7)'),
            textDisabled: color('rgba(255, 255, 255, 0.4)'),
            muted: color('#6B7280'),
            accent: color('rgba(59, 130, 246, 0.8)'),
            accentMuted: color('rgba(59, 130, 246, 0.3)'),
            accentSubtle: color('rgba(59, 130, 246, 0.15)'),
          },
        },
      },
      layerStyles: {
        card: {
          value: {
            bg: 'app.panel',
            borderWidth: '1px',
            borderColor: 'app.border',
            borderRadius: 'md',
          },
        },
        field: {
          value: {
            bg: 'app.panel',
            borderWidth: '1px',
            borderColor: 'app.border',
            color: 'white',
            fontSize: 'xs',
            _hover: { borderColor: 'app.borderStrong' },
            _focus: { borderColor: 'blue.400', boxShadow: 'none' },
            _placeholder: { color: 'app.placeholder' },
          },
        },
      },
    },
  }),
)

const queryClient = new QueryClient()

export function Provider({ children }: { children: ReactNode }) {
  return (
    <QueryClientProvider client={queryClient}>
      <ChakraProvider value={system}>
        <ThemeProvider
          attribute='class'
          forcedTheme='dark'
          disableTransitionOnChange
        >
          {children}
          <Toaster />
        </ThemeProvider>
      </ChakraProvider>
    </QueryClientProvider>
  )
}
