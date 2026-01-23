import type { LogEntry, LogFilter, LogLevel } from '../types'

export function filterLogs(entries: LogEntry[], filter: LogFilter): LogEntry[] {
  const { levels, modules, searchText } = filter
  const searchLower = searchText.toLowerCase().trim()

  return entries.filter(entry => {
    if (levels.length > 0) {
      if (!entry.is_parsed || !entry.level) {
        return false
      }
      if (!levels.includes(entry.level as LogLevel)) {
        return false
      }
    }

    if (modules.length > 0) {
      if (!entry.is_parsed || !entry.module) {
        return false
      }
      if (!modules.includes(entry.module)) {
        return false
      }
    }

    if (searchLower) {
      const matchesRaw = entry.raw.toLowerCase().includes(searchLower)
      const matchesMessage = entry.message.toLowerCase().includes(searchLower)
      const matchesModule =
        entry.module?.toLowerCase().includes(searchLower) ?? false

      if (!matchesRaw && !matchesMessage && !matchesModule) {
        return false
      }
    }

    return true
  })
}

export function extractModules(entries: LogEntry[]): string[] {
  const modules = new Set<string>()

  for (const entry of entries) {
    if (entry.is_parsed && entry.module) {
      modules.add(entry.module)
    }
  }

  return Array.from(modules).sort()
}

export function highlightText(
  text: string,
  searchText: string,
): Array<{ text: string; isMatch: boolean }> {
  if (!searchText.trim()) {
    return [{ text, isMatch: false }]
  }

  const searchLower = searchText.toLowerCase()
  const textLower = text.toLowerCase()
  const segments: Array<{ text: string; isMatch: boolean }> = []

  let lastIndex = 0
  let index = textLower.indexOf(searchLower)

  while (index !== -1) {
    if (index > lastIndex) {
      segments.push({
        text: text.slice(lastIndex, index),
        isMatch: false,
      })
    }

    segments.push({
      text: text.slice(index, index + searchText.length),
      isMatch: true,
    })

    lastIndex = index + searchText.length
    index = textLower.indexOf(searchLower, lastIndex)
  }

  if (lastIndex < text.length) {
    segments.push({
      text: text.slice(lastIndex),
      isMatch: false,
    })
  }

  return segments.length > 0 ? segments : [{ text, isMatch: false }]
}
