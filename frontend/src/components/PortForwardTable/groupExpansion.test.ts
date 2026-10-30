import { describe, expect, it } from 'vitest'

import {
  mergeVisible,
  NO_EXPANSION,
  parseSavedExpansion,
  pickVisible,
  savedExpanded,
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
})

describe('expansion of the shown groups', () => {
  it('shows only ids of groups on screen, in their order', () => {
    expect(pickVisible(['"b"', '"gone"', '"a"'], ['"a"', '"b"'])).toEqual([
      '"a"',
      '"b"',
    ])
  })

  it('replaces the state of shown groups and keeps the others', () => {
    expect(mergeVisible(['"a"', '"hidden"'], ['"a"', '"b"'], ['"b"'])).toEqual([
      '"hidden"',
      '"b"',
    ])
  })
})
