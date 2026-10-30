import { useCallback, useEffect, useMemo, useState } from 'react'

import {
  queryOptions,
  useMutation,
  useQuery,
  useQueryClient,
} from '@tanstack/react-query'

import {
  applyEdits,
  type ExpansionEdit,
  NO_EXPANSION,
  parseSavedExpansion,
  pickVisible,
  type SavedExpansion,
  type SearchCollapsed,
  savedExpanded,
  searchExpanded,
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
      }),
    ),
  staleTime: Number.POSITIVE_INFINITY,
  retry: false,
  meta: { errorToast: { title: 'Failed to load expanded groups' } },
})

export function useGroupExpansion(
  groups: ResolvedGroup[],
  groupBy: string | null,
  searching: boolean,
) {
  const queryClient = useQueryClient()
  const { data: saved } = useQuery(groupExpansionQuery)
  const [pendingEdits, setPendingEdits] = useState<ExpansionEdit[]>([])
  const [searchCollapsed, setSearchCollapsed] = useState<SearchCollapsed>({
    groupBy,
    ids: [],
  })
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
    if (saved === undefined || pendingEdits.length === 0) {
      return
    }
    const value = applyEdits(saved, pendingEdits)
    queryClient.setQueryData(groupExpansionQuery.queryKey, value)
    save(value)
    setPendingEdits([])
  }, [saved, pendingEdits, queryClient, save])

  useEffect(() => {
    setSearchCollapsed(prev => {
      const ids = searching ? pickVisible(prev.ids, groupIds) : []

      return ids.length === prev.ids.length ? prev : { ...prev, ids }
    })
  }, [searching, groupIds])

  const expanded = useMemo(
    () =>
      searching
        ? searchExpanded(searchCollapsed, groupBy, groupIds)
        : pickVisible(
            savedExpanded(
              applyEdits(saved ?? NO_EXPANSION, pendingEdits),
              groupBy,
            ),
            groupIds,
          ),
    [searching, searchCollapsed, saved, pendingEdits, groupBy, groupIds],
  )

  const setExpanded = useCallback(
    (next: string[]) => {
      if (searching) {
        setSearchCollapsed({
          groupBy,
          ids: groupIds.filter(id => !next.includes(id)),
        })

        return
      }
      setPendingEdits(prev => [...prev, { groupBy, groupIds, expanded: next }])
      if (saved === undefined) {
        void queryClient.refetchQueries(groupExpansionQuery)
      }
    },
    [searching, groupIds, groupBy, saved, queryClient],
  )

  return { expanded, setExpanded }
}
