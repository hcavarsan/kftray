const KB = 1024
const MB = KB * 1024

export function formatBytes(bytes: number) {
  if (bytes < KB) {
    return `${bytes} B`
  }
  if (bytes < MB) {
    return `${(bytes / KB).toFixed(1)} KB`
  }

  return `${(bytes / MB).toFixed(1)} MB`
}
