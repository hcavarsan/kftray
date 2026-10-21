import { describe, expect, it } from 'vitest'

import { makeConfig } from '@/components/Main/testFixtures'

import { visibleSelection } from './viewUtils'

describe('visibleSelection', () => {
  it('drops selected configs that the search or a filter hides', () => {
    const hidden = makeConfig(1)
    const visible = [makeConfig(2), makeConfig(3)]

    expect(visibleSelection(visible, [hidden])).toEqual([])
    expect(visibleSelection(visible, [hidden, makeConfig(3)])).toEqual([
      visible[1],
    ])
  })
})
