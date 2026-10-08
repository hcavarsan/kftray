const [command, args] = arguments
window.__TAURI__.core.invoke(command, args).catch(() => undefined)
return null
