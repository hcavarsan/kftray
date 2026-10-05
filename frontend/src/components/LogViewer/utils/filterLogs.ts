import type { LogEntry, LogFilter, RawLogEntry } from '../types'

/** `raw` holds the timestamp, level, module and message of every line. */
export function normalizeLogEntries(entries: RawLogEntry[]): LogEntry[] {
  return entries.map(entry => ({
    ...entry,
    searchable: entry.raw.toLowerCase(),
  }))
}

export function filterLogs(entries: LogEntry[], filter: LogFilter): LogEntry[] {
  const searchText = filter.searchText.trim().toLowerCase()
  const levels = new Set(filter.levels)
  const modules = new Set(filter.modules)

  return entries.filter(entry => {
    if (
      levels.size > 0 &&
      (!entry.is_parsed || !entry.level || !levels.has(entry.level))
    ) {
      return false
    }
    if (
      modules.size > 0 &&
      (!entry.is_parsed || !entry.module || !modules.has(entry.module))
    ) {
      return false
    }
    return !searchText || entry.searchable.includes(searchText)
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
  const normalizedSearchText = searchText.trim().toLowerCase()
  if (!normalizedSearchText) {
    return [{ text, isMatch: false }]
  }

  const normalizedText = text.toLowerCase()
  const segments: Array<{ text: string; isMatch: boolean }> = []
  let lastIndex = 0
  let index = normalizedText.indexOf(normalizedSearchText)

  while (index !== -1) {
    if (index > lastIndex) {
      segments.push({ text: text.slice(lastIndex, index), isMatch: false })
    }
    segments.push({
      text: text.slice(index, index + normalizedSearchText.length),
      isMatch: true,
    })
    lastIndex = index + normalizedSearchText.length
    index = normalizedText.indexOf(normalizedSearchText, lastIndex)
  }

  if (lastIndex < text.length) {
    segments.push({ text: text.slice(lastIndex), isMatch: false })
  }
  return segments.length > 0 ? segments : [{ text, isMatch: false }]
}
