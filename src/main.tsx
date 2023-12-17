import React from "react"
import ReactDOM from "react-dom/client"
import App from "./App"
import { ChakraProvider } from "@chakra-ui/react"
import theme from "./assets/theme" // Make sure this theme import path is correct
import "./assets/style.css"
import { attachConsole } from "tauri-plugin-log-api"
if (import.meta.env.DEV) {
  attachConsole()
}
ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <ChakraProvider theme={theme}>
      <App />
    </ChakraProvider>
  </React.StrictMode>,
)
