import { ALL_GROUP_ID } from '@/components/PortForwardTable/useConfigView'

export type SavedExpansion = Record<string, string[]>

export const NO_EXPANSION: SavedExpansion = {}

const isSavedExpansion = (value: unknown): value is SavedExpansion =>
  typeof value === 'object' &&
  value !== null &&
  !Array.isArray(value) &&
  Object.values(value).every(
    ids => Array.isArray(ids) && ids.every(id => typeof id === 'string'),
  )

export const parseSavedExpansion = (raw: string | null): SavedExpansion => {
  if (!raw) {
    return NO_EXPANSION
  }
  try {
    const parsed: unknown = JSON.parse(raw)

    return isSavedExpansion(parsed) ? parsed : NO_EXPANSION
  } catch {
    return NO_EXPANSION
  }
}

export const savedExpanded = (
  saved: SavedExpansion,
  groupBy: string | null,
): string[] => saved[groupBy ?? ''] ?? (groupBy === null ? [ALL_GROUP_ID] : [])

export const pickVisible = (ids: string[], groupIds: string[]) =>
  groupIds.filter(id => ids.includes(id))

export const mergeVisible = (
  previous: string[],
  groupIds: string[],
  visible: string[],
) => [...previous.filter(id => !groupIds.includes(id)), ...visible]

export const withExpanded = (
  saved: SavedExpansion,
  groupBy: string | null,
  groupIds: string[],
  expanded: string[],
): SavedExpansion => ({
  ...saved,
  [groupBy ?? '']: mergeVisible(
    savedExpanded(saved, groupBy),
    groupIds,
    expanded,
  ),
})
