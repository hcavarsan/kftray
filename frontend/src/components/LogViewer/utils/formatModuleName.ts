export function formatModuleName(module: string): string {
  const segments = module
    .replace(/^kftray_portforward::/, '')
    .replace(/^kftray_tauri::/, '')
    .replace(/^kftray_/, '')
    .split('::')

  if (segments.length > 3) {
    return `… ${segments.slice(-3).join(' › ')}`
  }

  return segments.join(' › ')
}
