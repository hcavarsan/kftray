import { createSystem, defineConfig } from '@chakra-ui/react'

const config = defineConfig({
  cssVarsRoot: ':where(:root, :host)',
  cssVarsPrefix: 'ck',
  theme: {
    semanticTokens: {
      colors: {
        // Background colors
        'bg.app': { value: '#161616' },
        'bg.subtle': { value: '#1A1A1A' },
        'bg.button': { value: 'rgba(255, 255, 255, 0.05)' },
        'bg.buttonHover': { value: 'rgba(255, 255, 255, 0.1)' },

        // Border colors
        'border.default': { value: 'rgba(255, 255, 255, 0.08)' },
        'border.subtle': { value: 'rgba(255, 255, 255, 0.04)' },

        // Text colors
        'text.primary': { value: 'rgba(255, 255, 255, 0.92)' },
        'text.secondary': { value: 'rgba(255, 255, 255, 0.64)' },
        'text.disabled': { value: 'rgba(255, 255, 255, 0.32)' },
      },
    },
    recipes: {
      Button: {
        base: {
          color: 'text.primary',
          bg: 'bg.button',
          borderColor: 'border.default',
          _hover: {
            bg: 'bg.buttonHover',
          },
          _disabled: {
            opacity: 0.4,
            color: 'text.disabled',
            cursor: 'not-allowed',
            _hover: {
              bg: 'bg.button',
            },
          },
        },
        variants: {
          ghost: {
            true: {
              bg: 'bg.button',
              color: 'text.primary',
              _hover: {
                bg: 'bg.buttonHover',
              },
            }
          },
        },
      },
    },
  },
})

export const system = createSystem(config)
