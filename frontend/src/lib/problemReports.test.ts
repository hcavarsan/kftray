import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

import * as Sentry from '@sentry/browser'

import { errorMessage } from './errors'
import {
  attachProblemReportContext,
  getProblemReportContext,
  submitProblemReport,
} from './problemReports'
import {
  captureInvokeFailure,
  scrub,
  setCrashReportsConsent,
} from './telemetry'

const native = vi.hoisted(() => ({ invoke: vi.fn() }))

vi.mock('@tauri-apps/api/core', () => ({ invoke: native.invoke }))

const receipts = new Map<string, (status: number) => void>()
const payloads: unknown[] = []

beforeEach(async () => {
  native.invoke.mockReset().mockResolvedValue(undefined)
  await setCrashReportsConsent(true)
  Sentry.init({
    dsn: 'https://public@example.invalid/1',
    defaultIntegrations: false,
    sendClientReports: false,
    dataCollection: { userInfo: false },
    beforeSend: scrub,
    transport: () => ({
      send: envelope => {
        const { promise, resolve } = Promise.withResolvers<{
          statusCode: number
        }>()
        const eventId = envelope[0].event_id
        if (typeof eventId !== 'string') {
          throw new Error('Missing event ID')
        }
        payloads.push(envelope[1][0][1])
        receipts.set(eventId, statusCode => resolve({ statusCode }))
        return promise
      },
      flush: async () => true,
    }),
  })
})

afterEach(async () => {
  for (const resolve of receipts.values()) {
    resolve(200)
  }
  await Sentry.close(2000)
  receipts.clear()
  payloads.length = 0
})

describe('reporting an invoke failure', () => {
  it('keeps concurrent failures associated with their own delivery result', async () => {
    const first = captureInvokeFailure('start_port_forward_tcp_cmd')
    const second = captureInvokeFailure('stop_port_forward_cmd')
    expect(first).toBeDefined()
    expect(second).toBeDefined()
    if (!first || !second) {
      throw new Error('Expected reportable failures')
    }
    const firstError = attachProblemReportContext('same native error', first)
    const secondError = attachProblemReportContext('same native error', second)
    await vi.waitFor(() => expect(receipts.size).toBe(2))
    receipts.get(second.eventId)?.(200)
    receipts.get(first.eventId)?.(503)

    expect(getProblemReportContext(firstError)?.eventId).toBe(first.eventId)
    expect(getProblemReportContext(secondError)?.eventId).toBe(second.eventId)
    await expect(first.delivery).resolves.toBe(false)
    await expect(second.delivery).resolves.toBe(true)
    expect(errorMessage(firstError)).toBe('same native error')
  })

  it('keeps native failure details out of the automatic event', async () => {
    const context = captureInvokeFailure('start_port_forward_tcp_cmd')
    const original = new Error('secret-token at prod/cluster')
    const failure = attachProblemReportContext(original, context)
    await vi.waitFor(() => expect(receipts.size).toBe(1))

    expect(failure).toBe(original)
    expect(JSON.stringify(payloads)).not.toContain('secret-token')
    expect(JSON.stringify(payloads)).not.toContain('prod/cluster')
    expect(payloads).toEqual([
      expect.objectContaining({
        tags: { command: 'start_port_forward_tcp_cmd' },
        exception: {
          values: [expect.objectContaining({ type: 'InvokeError' })],
        },
      }),
    ])
  })

  it('keeps the rejection unchanged when automatic reporting is off', async () => {
    await setCrashReportsConsent(false)
    const context = captureInvokeFailure('start_port_forward_tcp_cmd')

    expect(context).toBeUndefined()
    expect(attachProblemReportContext('native failure', context)).toBe(
      'native failure',
    )
    expect(receipts.size).toBe(0)
  })

  it('does not submit feedback when the associated error was rejected', async () => {
    const context = captureInvokeFailure('start_port_forward_tcp_cmd')
    if (!context) {
      throw new Error('Expected a reportable failure')
    }
    await vi.waitFor(() => expect(receipts.size).toBe(1))
    receipts.get(context.eventId)?.(429)
    native.invoke.mockClear()

    await expect(
      submitProblemReport({
        reportId: crypto.randomUUID(),
        message: 'The forward could not start.',
        context,
      }),
    ).rejects.toThrow('The error could not be delivered')
    expect(native.invoke).not.toHaveBeenCalled()
  })
})
