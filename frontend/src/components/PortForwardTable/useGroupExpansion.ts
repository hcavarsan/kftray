import { useCallback, useEffect, useMemo, useState } from 'react'

import {
  queryOptions,
  useMutation,
  useQuery,
  useQueryClient,
} from '@tanstack/react-query'

import {
  mergeVisible,
  NO_EXPANSION,
  parseSavedExpansion,
  pickVisible,
  type SavedExpansion,
  savedExpanded,
  withExpanded,
} from '@/components/PortForwardTable/groupExpansion'
import { invoke } from '@/lib/tauri'
import type { ResolvedGroup } from '@/types'

const GROUP_EXPANSION_SETTING = 'group_expansion'

const groupExpansionQuery = queryOptions({
  queryKey: ['setting', GROUP_EXPANSION_SETTING],
  queryFn: async () =>
    parseSavedExpansion(
      await invoke<string | null>('get_setting_value', {
        key: GROUP_EXPANSION_SETTING,
      }).catch(() => null),
    ),
  staleTime: Number.POSITIVE_INFINITY,
})

export function useGroupExpansion(
  groups: ResolvedGroup[],
  groupBy: string | null,
  searching: boolean,
) {
  const queryClient = useQueryClient()
  const { data: saved = NO_EXPANSION } = useQuery(groupExpansionQuery)
  const [searchCollapsed, setSearchCollapsed] = useState<string[]>([])
  const groupIds = useMemo(() => groups.map(group => group.id), [groups])

  const { mutate: save } = useMutation({
    mutationFn: (value: SavedExpansion) =>
      invoke('set_setting_value', {
        key: GROUP_EXPANSION_SETTING,
        value: JSON.stringify(value),
      }),
    scope: { id: GROUP_EXPANSION_SETTING },
    meta: { errorToast: { title: 'Failed to save expanded groups' } },
  })

  useEffect(() => {
    if (!searching) {
      setSearchCollapsed([])
    }
  }, [searching])

  const expanded = useMemo(
    () =>
      searching
        ? groupIds.filter(id => !searchCollapsed.includes(id))
        : pickVisible(savedExpanded(saved, groupBy), groupIds),
    [searching, searchCollapsed, saved, groupBy, groupIds],
  )

  const setExpanded = useCallback(
    (next: string[]) => {
      if (searching) {
        setSearchCollapsed(prev =>
          mergeVisible(
            prev,
            groupIds,
            groupIds.filter(id => !next.includes(id)),
          ),
        )

        return
      }
      void queryClient.ensureQueryData(groupExpansionQuery).then(() => {
        const value = withExpanded(
          queryClient.getQueryData(groupExpansionQuery.queryKey) ??
            NO_EXPANSION,
          groupBy,
          groupIds,
          next,
        )
        queryClient.setQueryData(groupExpansionQuery.queryKey, value)
        save(value)
      })
    },
    [searching, groupIds, groupBy, queryClient, save],
  )

  return { expanded, setExpanded }
}
