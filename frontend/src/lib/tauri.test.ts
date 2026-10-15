import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import * as Sentry from '@sentry/browser'

import { showErrorToast } from '@/components/ui/error-toast'
import { toaster } from '@/components/ui/toaster'

import { errorMessage } from './errors'
import { getProblemReportContext } from './problemReports'
import { invoke } from './tauri'
import { setCrashReportsConsent } from './telemetry'

const native = vi.hoisted(() => ({ invoke: vi.fn() }))

vi.mock('@tauri-apps/api/core', () => ({ invoke: native.invoke }))
vi.mock('@/components/ui/toaster', () => ({ toaster: { error: vi.fn() } }))

const reject = async (command: string): Promise<unknown> => {
  try {
    await invoke(command)
  } catch (error) {
    return error
  }
  throw new Error('Expected the invoke to reject')
}

const sentEvents: unknown[] = []

beforeEach(async () => {
  sentEvents.length = 0
  native.invoke.mockReset()
  vi.mocked(toaster.error).mockClear()
  await setCrashReportsConsent(true)
  Sentry.init({
    dsn: 'https://public@example.invalid/1',
    defaultIntegrations: false,
    sendClientReports: false,
    transport: () => ({
      send: async envelope => {
        sentEvents.push(envelope[1][0][1])
        return { statusCode: 200 }
      },
      flush: async () => true,
    }),
  })
})

afterEach(async () => {
  vi.restoreAllMocks()
  await Sentry.close(2000)
})

describe('invoke rejections', () => {
  it('links a failure the backend already reported without reporting it again', async () => {
    native.invoke.mockRejectedValue({
      message: 'Port 8080 is already in use',
      eventId: '0a1b2c3d-0000-4000-8000-000000000000',
    })

    const error = await reject('start_port_forward_tcp_cmd')
    await Sentry.flush(2000)

    expect(sentEvents).toHaveLength(0)
    expect(getProblemReportContext(error)).toEqual({
      eventId: '0a1b2c3d-0000-4000-8000-000000000000',
    })
    expect(errorMessage(error)).toBe('Port 8080 is already in use')
  })

  it('does not report a failure the backend measured and chose not to send', async () => {
    native.invoke.mockRejectedValue({
      message: 'Port forwarding is already running for config 1',
      eventId: null,
    })

    const error = await reject('start_port_forward_tcp_cmd')
    await Sentry.flush(2000)

    expect(sentEvents).toHaveLength(0)
    expect(getProblemReportContext(error)).toBeUndefined()
    expect(errorMessage(error)).toBe(
      'Port forwarding is already running for config 1',
    )
  })

  it('still shows the backend message in the error toast', async () => {
    native.invoke.mockRejectedValue({
      message: 'Port 8080 is already in use',
      eventId: '0a1b2c3d-0000-4000-8000-000000000000',
    })

    const error = await reject('start_port_forward_tcp_cmd')
    showErrorToast(error, { errorToast: { title: 'Failed to start' } })

    expect(toaster.error).toHaveBeenCalledWith(
      expect.objectContaining({
        description: 'Port 8080 is already in use',
        action: expect.objectContaining({ label: 'Report' }),
      }),
    )
  })

  it('still reports a plain string rejection', async () => {
    native.invoke.mockRejectedValue('Config not found')
    const error = await reject('get_config_cmd')
    await Sentry.flush(2000)

    expect(sentEvents).toHaveLength(1)
    expect(getProblemReportContext(error)?.eventId).toBeDefined()
    expect(errorMessage(error)).toBe('Config not found')
  })
})
