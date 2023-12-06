// theme.tsx
import { extendTheme, ThemeConfig } from "@chakra-ui/react";

const config: ThemeConfig = {
	initialColorMode: 'dark',
	useSystemColorMode: false,
  };


  const theme = extendTheme({
	config,
	colors: {
	  dark: {
		700: "#2D3748", // A dark gray background color
	  },
	  purple: {
		// Purple variations
		500: "#805AD5",
		600: "#6B46C1",
	  },
	},
});

export default theme;
