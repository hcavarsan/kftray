import { afterEach, describe, expect, it } from 'vitest'

import { showErrorToast } from './error-toast'
import { toaster } from './toaster'

describe('showErrorToast', () => {
  afterEach(() => {
    toaster.remove()
  })

  it('gives a toast without a meta id its own id', () => {
    showErrorToast(new Error('Invalid config'), {
      errorToast: { title: 'Failed to import configs' },
    })

    const toast = toaster
      .getVisibleToasts()
      .find(visible => visible.title === 'Failed to import configs')

    expect(toast?.id).toEqual(expect.any(String))
    expect(toast?.description).toBe('Invalid config')
  })

  it('keeps the meta id so a repeated error replaces its toast', () => {
    const meta = { errorToast: { id: 'sync-error', title: 'Sync failed' } }

    showErrorToast(new Error('first'), meta)
    showErrorToast(new Error('second'), meta)

    const toasts = toaster
      .getVisibleToasts()
      .filter(toast => toast.title === 'Sync failed')

    expect(toasts).toHaveLength(1)
    expect(toasts[0]?.id).toBe('sync-error')
    expect(toasts[0]?.description).toBe('second')
  })
})
