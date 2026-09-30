import { describe, expect, it } from 'vitest'

import type { ConfigDraft } from './types'
import { applyDraftChange } from './utils'

const draft: ConfigDraft = {
  context: 'ctx',
  kubeconfig: '/kube/a',
  namespace: 'ns',
  remote_port: '8080',
  service: 'svc',
  target: 'app=web',
  workload_type: 'service',
}

describe('applyDraftChange', () => {
  it('clears everything below a kubeconfig change', () => {
    expect(applyDraftChange(draft, { kubeconfig: '/kube/b' })).toMatchObject({
      context: '',
      kubeconfig: '/kube/b',
      namespace: '',
      remote_port: '',
      service: '',
      target: '',
    })
  })

  it('keeps namespace out of a context change only when unchanged', () => {
    expect(applyDraftChange(draft, { context: 'other' })).toMatchObject({
      context: 'other',
      namespace: '',
      remote_port: '',
      service: '',
      target: '',
    })
    expect(applyDraftChange(draft, { context: 'ctx' })).toEqual(draft)
  })

  it('clears the target and port on namespace and workload changes but keeps context', () => {
    for (const change of [
      { namespace: 'x' },
      { workload_type: 'pod' },
    ] as const) {
      expect(applyDraftChange(draft, change)).toMatchObject({
        context: 'ctx',
        remote_port: '',
        service: '',
        target: '',
      })
    }
  })

  it('clears only the port on service or target changes', () => {
    expect(applyDraftChange(draft, { service: 'other' })).toMatchObject({
      namespace: 'ns',
      remote_port: '',
      service: 'other',
      target: 'app=web',
    })
    expect(applyDraftChange(draft, { target: 'app=x' })).toMatchObject({
      remote_port: '',
      service: 'svc',
      target: 'app=x',
    })
  })

  it('lets explicit values in the same change win over the cascade', () => {
    expect(
      applyDraftChange(draft, { namespace: 'x', remote_port: '9000' }),
    ).toMatchObject({ namespace: 'x', remote_port: '9000', service: '' })
  })

  it('does not touch the port for unrelated changes', () => {
    expect(applyDraftChange(draft, { alias: 'a' }).remote_port).toBe('8080')
  })
})
