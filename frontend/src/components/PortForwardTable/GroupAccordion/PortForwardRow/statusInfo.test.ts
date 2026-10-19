import { describe, expect, it } from 'vitest'

import { makeConfig } from '@/components/Main/testFixtures'
import type { Config } from '@/types'

import { getConfigDetails, getStatusInfo } from './statusInfo'

const failing: Config = {
  ...makeConfig(1, true),
  last_error: 'pod web-0 is not ready',
}

describe('getStatusInfo', () => {
  it('marks a running forward whose connections fail', () => {
    const status = getStatusInfo(failing, null, 'web-0')

    expect(status).toMatchObject({ status: 'Failing', color: 'warning.fg' })
  })

  it('shows a reconnect before the error it is recovering from', () => {
    const status = getStatusInfo(
      { ...failing, is_retrying: true, retry_count: 2 },
      null,
      'web-0',
    )

    expect(status).toMatchObject({
      status: 'Reconnecting',
      description: 'Reconnect attempt 2',
    })
  })

  it('keeps the warning on a forward that stopped after an error', () => {
    const status = getStatusInfo({ ...failing, is_running: false }, null, null)

    expect(status).toMatchObject({ status: 'Stopped', color: 'warning.fg' })
  })

  it('lets a pending start win over an old error', () => {
    const status = getStatusInfo(
      { ...failing, is_running: false },
      { action: 'starting', token: 1 },
      null,
    )

    expect(status.status).toBe('Starting')
  })

  it('shows a healthy running forward as running', () => {
    const status = getStatusInfo(makeConfig(1, true), null, 'web-0')

    expect(status.status).toBe('Running')
  })

  it('shows a forward another kftray process runs as running elsewhere', () => {
    const status = getStatusInfo(
      { ...makeConfig(1, true), owned_elsewhere: true },
      null,
      null,
    )

    expect(status).toEqual({
      color: 'fg.muted',
      status: 'Running elsewhere',
      description: 'Running in another kftray process',
    })
  })
})

describe('getConfigDetails', () => {
  it('lists the reconnect attempt and the last error', () => {
    const details = getConfigDetails(
      { ...failing, is_retrying: true, retry_count: 3 },
      'web-0',
    )

    expect(details).toEqual(
      expect.arrayContaining([
        { label: 'Reconnect Attempt', value: '3' },
        { label: 'Last Error', value: 'pod web-0 is not ready' },
      ]),
    )
  })
})

describe('getConfigDetails for expose', () => {
  const expose: Config = {
    alias: 'db',
    exposure_type: 'cluster',
    id: 2,
    is_running: true,
    local_port: 5432,
    namespace: 'ns',
    workload_type: 'expose',
  }

  it('shows TCP for a tcp cluster expose', () => {
    const details = getConfigDetails(
      { ...expose, exposure_protocol: 'tcp' },
      null,
    )

    expect(details).toEqual(
      expect.arrayContaining([{ label: 'Protocol', value: 'TCP' }]),
    )
    expect(details).not.toContainEqual(
      expect.objectContaining({ value: expect.stringContaining('http://') }),
    )
  })

  it('shows HTTP for a cluster expose without a protocol', () => {
    expect(getConfigDetails(expose, null)).toEqual(
      expect.arrayContaining([{ label: 'Protocol', value: 'HTTP' }]),
    )
  })

  it('shows no protocol row for a public expose', () => {
    const details = getConfigDetails(
      { ...expose, exposure_type: 'public' },
      null,
    )

    expect(details.some(detail => detail.label === 'Protocol')).toBe(false)
  })
})
