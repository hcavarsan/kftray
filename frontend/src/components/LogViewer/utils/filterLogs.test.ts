import { describe, expect, it } from 'vitest'

import type { RawLogEntry } from '../types'

import { filterLogs, highlightText, normalizeLogEntries } from './filterLogs'

const entry = (id: number, overrides: Partial<RawLogEntry>): RawLogEntry => ({
  id,
  raw: '',
  timestamp: null,
  date: null,
  time: null,
  level: null,
  module: null,
  message: '',
  is_parsed: true,
  ...overrides,
})

const entries = normalizeLogEntries([
  entry(1, {
    raw: '10:00 ERROR kftray::kube boom',
    level: 'ERROR',
    module: 'kftray::kube',
  }),
  entry(2, {
    raw: '10:01 INFO kftray::kube Started',
    level: 'INFO',
    module: 'kftray::kube',
  }),
  entry(3, {
    raw: '10:02 ERROR kftray::ssh refused',
    level: 'ERROR',
    module: 'kftray::ssh',
  }),
  entry(4, { raw: 'thread panicked ERROR', is_parsed: false }),
])

const ids = (filtered: { id: number }[]) => filtered.map(e => e.id)

describe('filterLogs', () => {
  it('keeps only parsed lines whose level and module are both selected', () => {
    expect(
      ids(
        filterLogs(entries, { levels: ['ERROR'], modules: [], searchText: '' }),
      ),
    ).toEqual([1, 3])
    expect(
      ids(
        filterLogs(entries, {
          levels: ['ERROR'],
          modules: ['kftray::kube'],
          searchText: '',
        }),
      ),
    ).toEqual([1])
  })

  it('matches search text ignoring case and surrounding spaces', () => {
    expect(
      ids(
        filterLogs(entries, {
          levels: [],
          modules: [],
          searchText: '  STARTED ',
        }),
      ),
    ).toEqual([2])
    expect(
      ids(filterLogs(entries, { levels: [], modules: [], searchText: '   ' })),
    ).toEqual([1, 2, 3, 4])
  })
})

describe('highlightText', () => {
  it('marks every case insensitive match and keeps the original text', () => {
    expect(highlightText('Error then error', ' ERROR ')).toEqual([
      { text: 'Error', isMatch: true },
      { text: ' then ', isMatch: false },
      { text: 'error', isMatch: true },
    ])
  })
})
