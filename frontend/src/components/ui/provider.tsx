'use client'

import {
  ChakraProvider,
  createSystem,
  defaultSystem,
  defineConfig,
} from '@chakra-ui/react'

import { ColorModeProvider, type ColorModeProviderProps } from './color-mode'

const system = createSystem(
  defineConfig({
    ...defaultSystem._config,
    globalCss: {
      body: {
        backgroundColor: 'transparent',
        margin: 0,
      },
    },
  }),
)

export function Provider(props: ColorModeProviderProps) {
  return (
    <ChakraProvider value={system}>
      <ColorModeProvider {...props} />
    </ChakraProvider>
  )
}
