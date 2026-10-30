import { describe, expect, it } from 'vitest'

import {
  applyEdits,
  NO_EXPANSION,
  parseSavedExpansion,
  pickVisible,
  savedExpanded,
  searchExpanded,
  withExpanded,
} from './groupExpansion'
import { ALL_GROUP_ID } from './useConfigView'

describe('saved group expansion', () => {
  it('opens the groups the user left open after a restart', () => {
    const saved = withExpanded(
      NO_EXPANSION,
      'context',
      ['"dev"', '"prod"'],
      ['"prod"'],
    )

    expect(
      savedExpanded(parseSavedExpansion(JSON.stringify(saved)), 'context'),
    ).toEqual(['"prod"'])
  })

  it('keeps each grouping its own state', () => {
    const byContext = withExpanded(NO_EXPANSION, 'context', ['"dev"'], [])
    const both = withExpanded(byContext, 'tag:team', ['"core"'], ['"core"'])

    expect(savedExpanded(both, 'context')).toEqual([])
    expect(savedExpanded(both, 'tag:team')).toEqual(['"core"'])
  })

  it('opens the single group without grouping and closes the rest by default', () => {
    expect(savedExpanded(NO_EXPANSION, null)).toEqual([ALL_GROUP_ID])
    expect(savedExpanded(NO_EXPANSION, 'namespace')).toEqual([])
    expect(
      savedExpanded(withExpanded(NO_EXPANSION, null, [ALL_GROUP_ID], []), null),
    ).toEqual([])
  })

  it('keeps groups a filter hides open when the visible ones change', () => {
    const saved = { context: ['"dev"', '"prod"'] }
    const next = withExpanded(saved, 'context', ['"dev"', '"qa"'], [])

    expect(savedExpanded(next, 'context')).toEqual(['"prod"'])
    expect(
      pickVisible(savedExpanded(next, 'context'), ['"dev"', '"qa"']),
    ).toEqual([])
  })

  it('ignores a stored value it cannot read', () => {
    expect(parseSavedExpansion(null)).toEqual({})
    expect(parseSavedExpansion('not json')).toEqual({})
    expect(parseSavedExpansion('["a"]')).toEqual({})
    expect(parseSavedExpansion('{"context":[1]}')).toEqual({})
  })

  it('applies choices made while the saved state could not be read', () => {
    const saved = { context: ['"qa"'], 'tag:team': ['"core"'] }
    const next = applyEdits(saved, [
      {
        groupBy: 'context',
        groupIds: ['"dev"', '"prod"'],
        expanded: ['"dev"'],
      },
      { groupBy: 'namespace', groupIds: ['"default"'], expanded: [] },
    ])

    expect(next).toEqual({
      context: ['"qa"', '"dev"'],
      namespace: [],
      'tag:team': ['"core"'],
    })
  })
})

describe('expansion of the shown groups', () => {
  it('shows only ids of groups on screen, in their order', () => {
    expect(pickVisible(['"b"', '"gone"', '"a"'], ['"a"', '"b"'])).toEqual([
      '"a"',
      '"b"',
    ])
  })
})

describe('expansion during a search', () => {
  const groupIds = ['"dev"', '"prod"']

  it('opens a collapsed match again once it stopped matching', () => {
    const collapsed = { groupBy: 'context', ids: ['"prod"'] }
    const pruned = { ...collapsed, ids: pickVisible(collapsed.ids, ['"dev"']) }

    expect(searchExpanded(pruned, 'context', groupIds)).toEqual(groupIds)
  })

  it('does not carry a collapse over to another grouping', () => {
    const collapsed = { groupBy: 'context', ids: ['"prod"'] }

    expect(searchExpanded(collapsed, 'namespace', groupIds)).toEqual(groupIds)
    expect(searchExpanded(collapsed, 'context', groupIds)).toEqual(['"dev"'])
  })
})
