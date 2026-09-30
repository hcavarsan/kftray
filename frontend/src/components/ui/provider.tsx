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
  MutationCache,
  QueryCache,
  QueryClient,
  QueryClientProvider,
} from '@tanstack/react-query'

import { errorMessage } from '@/lib/errors'

import { Toaster, toaster } from './toaster'

interface ErrorToastMeta {
  errorToast?: {
    id?: string
    title: string
    description?: string
    duration?: number
  }
}

declare module '@tanstack/react-query' {
  interface Register {
    queryMeta: ErrorToastMeta
    mutationMeta: ErrorToastMeta
  }
}

const showErrorToast = (error: unknown, meta: ErrorToastMeta | undefined) => {
  const toast = meta?.errorToast

  if (toast) {
    toaster.error({
      id: toast.id,
      title: toast.title,
      description: toast.description ?? errorMessage(error),
      duration: toast.duration,
    })
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
          bg: {
            canvas: color('#111111'),
            deep: color('#0a0a0a'),
            surface: color('#161616'),
            raised: color('#1a1a1a'),
            faint: color('rgba(255, 255, 255, 0.04)'),
            hover: color('rgba(255, 255, 255, 0.06)'),
            active: color('rgba(255, 255, 255, 0.1)'),
            shade: color('rgba(0, 0, 0, 0.2)'),
            scrim: color('rgba(0, 0, 0, 0.8)'),
          },
          fg: {
            DEFAULT: color('#ffffff'),
            secondary: color('#d4d4d8'),
            muted: color('#a1a1aa'),
            subtle: color('rgba(255, 255, 255, 0.48)'),
            faint: color('rgba(255, 255, 255, 0.24)'),
          },
          border: {
            DEFAULT: color('rgba(255, 255, 255, 0.08)'),
            subtle: color('rgba(255, 255, 255, 0.05)'),
            emphasized: color('rgba(255, 255, 255, 0.15)'),
            strong: color('rgba(255, 255, 255, 0.2)'),
          },
          accent: {
            fg: color('#60a5fa'),
            secondary: color('#c084fc'),
            solid: color('#3b82f6'),
            solidHover: color('#2563eb'),
            solidActive: color('#1d4ed8'),
            focusRing: color('#60a5fa'),
            subtle: color('rgba(59, 130, 246, 0.12)'),
            muted: color('rgba(59, 130, 246, 0.25)'),
            emphasized: color('rgba(59, 130, 246, 0.45)'),
          },
          danger: {
            fg: color('#fca5a5'),
            solid: color('#ef4444'),
            emphasized: color('#dc2626'),
            subtle: color('rgba(229, 62, 62, 0.1)'),
            muted: color('rgba(229, 62, 62, 0.2)'),
          },
          success: {
            fg: color('#4ade80'),
            subtle: color('rgba(56, 161, 105, 0.1)'),
            border: color('rgba(56, 161, 105, 0.2)'),
          },
          warning: {
            fg: color('#fdba74'),
            subtle: color('#3b1106'),
            border: color('#fb923c'),
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
            module: {
              fg: color('#67e8f9'),
              solid: color('#67e8f9'),
              contrast: color('#000000'),
              subtle: color('rgba(34, 211, 238, 0.1)'),
              muted: color('rgba(34, 211, 238, 0.2)'),
              emphasized: color('rgba(34, 211, 238, 0.3)'),
            },
          },
          status: {
            unresponsive: color('rgba(217, 119, 6, 0.7)'),
            busy: color('rgba(100, 116, 139, 0.6)'),
            rollout: color('rgba(161, 98, 7, 0.7)'),
            stopped: color('rgba(100, 116, 139, 0.4)'),
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
            bg: 'bg.surface',
            borderWidth: '1px',
            borderColor: 'border',
            borderRadius: 'md',
          },
        },
        field: {
          value: {
            bg: 'bg.surface',
            borderWidth: '1px',
            borderColor: 'border',
            color: 'fg',
            fontSize: 'xs',
            _hover: { borderColor: 'border.emphasized' },
            _focus: { borderColor: 'accent.focusRing', boxShadow: 'none' },
            _placeholder: { color: 'fg.subtle' },
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
    onError: (error, query) => showErrorToast(error, query.meta),
  }),
  mutationCache: new MutationCache({
    onError: (error, _variables, _context, mutation) =>
      showErrorToast(error, mutation.meta),
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
