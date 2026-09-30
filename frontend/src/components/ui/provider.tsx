import type { ReactNode } from 'react'

import {
  ChakraProvider,
  createSystem,
  defaultBaseConfig,
  defineConfig,
} from '@chakra-ui/react'
import {
  accordionSlotRecipe,
  animationStyles,
  badgeRecipe,
  breakpoints,
  buttonRecipe,
  checkboxSlotRecipe,
  checkmarkRecipe,
  cssVarsPrefix,
  cssVarsRoot,
  dialogSlotRecipe,
  fieldSlotRecipe,
  inputRecipe,
  keyframes,
  layerStyles,
  menuSlotRecipe,
  progressSlotRecipe,
  radioGroupSlotRecipe,
  radiomarkRecipe,
  semanticTokens,
  sliderSlotRecipe,
  spinnerRecipe,
  switchSlotRecipe,
  tableSlotRecipe,
  textStyles,
  toastSlotRecipe,
  tokens,
  tooltipSlotRecipe,
} from '@chakra-ui/react/theme'
import {
  QueryCache,
  QueryClient,
  QueryClientProvider,
} from '@tanstack/react-query'

import { errorMessage } from '@/lib/errors'

import { Toaster, toaster } from './toaster'

declare module '@tanstack/react-query' {
  interface Register {
    queryMeta: {
      errorToast?: {
        id?: string
        title: string
        description?: string
        duration?: number
      }
    }
  }
}

const color = (value: string) => ({ value })

const system = createSystem(
  defaultBaseConfig,
  defineConfig({
    preflight: true,
    cssVarsPrefix,
    cssVarsRoot,
    theme: {
      breakpoints,
      keyframes,
      tokens,
      semanticTokens,
      textStyles,
      layerStyles,
      animationStyles,
      recipes: {
        badge: badgeRecipe,
        button: buttonRecipe,
        checkmark: checkmarkRecipe,
        input: inputRecipe,
        radiomark: radiomarkRecipe,
        spinner: spinnerRecipe,
      },
      slotRecipes: {
        accordion: accordionSlotRecipe,
        checkbox: checkboxSlotRecipe,
        dialog: dialogSlotRecipe,
        field: fieldSlotRecipe,
        menu: menuSlotRecipe,
        progress: progressSlotRecipe,
        radioGroup: radioGroupSlotRecipe,
        slider: sliderSlotRecipe,
        switch: switchSlotRecipe,
        table: tableSlotRecipe,
        toast: toastSlotRecipe,
        tooltip: tooltipSlotRecipe,
      },
    },
  }),
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
            accentText: color('rgba(147, 197, 253, 1)'),
            accentTextMuted: color('rgba(147, 197, 253, 0.8)'),
            checked: color('#3182CE'),
            shade: color('rgba(0, 0, 0, 0.2)'),
            scrim: color('rgba(0, 0, 0, 0.8)'),
          },
          log: {
            error: {
              bg: color('rgba(229, 62, 62, 0.15)'),
              text: color('rgba(252, 129, 129, 1)'),
              border: color('rgba(229, 62, 62, 0.3)'),
            },
            warn: {
              bg: color('rgba(161, 98, 7, 0.15)'),
              text: color('rgba(251, 191, 36, 1)'),
              border: color('rgba(161, 98, 7, 0.3)'),
            },
            debug: {
              bg: color('rgba(139, 92, 246, 0.15)'),
              text: color('rgba(196, 181, 253, 1)'),
              border: color('rgba(139, 92, 246, 0.3)'),
            },
            trace: {
              bg: color('rgba(100, 116, 139, 0.15)'),
              text: color('rgba(148, 163, 184, 1)'),
              border: color('rgba(100, 116, 139, 0.3)'),
            },
          },
          status: {
            unresponsive: color('rgba(217, 119, 6, 0.7)'),
            busy: color('rgba(100, 116, 139, 0.6)'),
            rollout: color('rgba(161, 98, 7, 0.7)'),
            stopped: color('rgba(100, 116, 139, 0.4)'),
            success: {
              bg: color('rgba(56, 161, 105, 0.1)'),
              border: color('rgba(56, 161, 105, 0.2)'),
            },
            danger: {
              bg: color('rgba(229, 62, 62, 0.1)'),
              border: color('rgba(229, 62, 62, 0.2)'),
            },
            warning: {
              border: color('rgba(255, 165, 0, 0.3)'),
            },
          },
          search: {
            bg: color('rgba(251, 191, 36, 0.1)'),
            border: color('rgba(251, 191, 36, 0.2)'),
            match: color('rgba(251, 191, 36, 0.3)'),
          },
        },
        shadows: {
          popover: color('0 8px 24px rgba(0, 0, 0, 0.45)'),
          dialog: color(
            '0 20px 25px -5px rgba(0, 0, 0, 0.3), 0 10px 10px -5px rgba(0, 0, 0, 0.1)',
          ),
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

const queryClient = new QueryClient({
  defaultOptions: {
    queries: { networkMode: 'always' },
    mutations: { networkMode: 'always' },
  },
  queryCache: new QueryCache({
    onError: (error, query) => {
      const toast = query.meta?.errorToast

      if (toast) {
        toaster.error({
          id: toast.id,
          title: toast.title,
          description: toast.description ?? errorMessage(error),
          duration: toast.duration,
        })
      }
    },
  }),
})

export function Provider({ children }: { children: ReactNode }) {
  return (
    <QueryClientProvider client={queryClient}>
      <ChakraProvider value={system}>
        {children}
        <Toaster />
      </ChakraProvider>
    </QueryClientProvider>
  )
}
