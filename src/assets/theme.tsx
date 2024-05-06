import { extendTheme, ThemeConfig } from '@chakra-ui/react'

const config: ThemeConfig = {
  initialColorMode: 'dark',
  useSystemColorMode: false,
}

const theme = extendTheme({
  config,
  components: {
    Toast: {
      baseStyle: {
        maxWidth: '300px',
        fontSize: 'xs',
        mt: '3',
      },
      variants: {
        error: {
          bg: 'red.800',
          color: 'white',
        },
        success: {
          bg: 'green.600',
          color: 'white',
        },
        // Add more variants as needed
      },
      defaultProps: {
        variant: 'error',
      },
    },
  },
  styles: {
    global: {
      body: {
        margin: 0,
        backgroundColor: 'transparent',
      },
    },
  },
})

export default theme
