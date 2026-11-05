import { describe, expect, it } from 'vitest'

import { makeConfig } from '@/components/Main/testFixtures'
import type { Config } from '@/types'

import { buildLocalUrl } from './buildLocalUrl'

describe.each([
  { ...makeConfig(1, true), workload_type: 'service', service: 'web' },
  {
    id: 2,
    alias: 'web',
    local_port: 8080,
    is_running: true,
    workload_type: 'expose',
    exposure_type: 'cluster',
    exposure_protocol: 'http',
  },
] satisfies Config[])('buildLocalUrl for $workload_type', config => {
  it.each([
    ['::1', 'http://[::1]:8080'],
    ['2001:db8::1', 'http://[2001:db8::1]:8080'],
    ['::ffff:127.0.0.1', 'http://[::ffff:127.0.0.1]:8080'],
    ['[::1]', 'http://[::1]:8080'],
    ['127.0.0.1', 'http://127.0.0.1:8080'],
    ['localhost', 'http://localhost:8080'],
  ])('builds a browser URL for %s', (address, expected) => {
    expect(
      buildLocalUrl({ ...config, local_address: address, local_port: 8080 }),
    ).toBe(expected)
  })
})

describe('buildLocalUrl host selection', () => {
  it('uses the domain alias instead of the IPv6 bind address', () => {
    expect(
      buildLocalUrl({
        ...makeConfig(1, true),
        alias: 'web.local',
        domain_enabled: true,
        local_address: '::1',
        local_port: 8080,
      }),
    ).toBe('http://web.local:8080')
  })

  it.each([
    [false, 'http://public.example.com'],
    [true, 'https://public.example.com'],
  ])('uses the public expose URL with TLS %s', (tlsEnabled, expected) => {
    expect(
      buildLocalUrl({
        ...makeConfig(1, true),
        workload_type: 'expose',
        exposure_type: 'public',
        alias: 'public.example.com',
        cert_manager_enabled: tlsEnabled,
        local_address: '::1',
        local_port: 8080,
      }),
    ).toBe(expected)
  })
})
