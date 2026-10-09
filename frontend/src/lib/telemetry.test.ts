import { beforeEach, describe, expect, it, vi } from 'vitest'

import type { ErrorEvent } from '@sentry/browser'

import {
  initCrashReporting,
  scrub,
  scrubTransaction,
  type TransactionEvent,
} from './telemetry'

const MAIN = 'tauri://localhost/assets/main-BkQTPf31.js'
const WINDOWS_MAIN = 'https://tauri.localhost/assets/main-BkQTPf31.js'

const event = (overrides: Partial<ErrorEvent> = {}): ErrorEvent => ({
  type: undefined,
  event_id: 'e1',
  timestamp: 1,
  level: 'error',
  platform: 'javascript',
  release: 'kftray@0.30.0',
  environment: 'production',
  tags: { app: 'kftray', surface: 'webview', run_id: 'r1' },
  debug_meta: {
    images: [
      {
        type: 'sourcemap',
        code_file: MAIN,
        debug_id: 'aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee',
      },
    ],
  },
  request: {
    url: 'tauri://localhost/index.html',
    headers: { 'User-Agent': 'Mozilla/5.0 WebKit' },
  },
  breadcrumbs: [{ category: 'console', message: 'forwarding prod/api' }],
  contexts: { trace: { trace_id: 't', span_id: 's' } },
  extra: { kubeconfig: '/home/me/.kube/config' },
  user: { ip_address: '{{auto}}' },
  exception: {
    values: [
      {
        type: 'Error',
        value: 'Failed to forward https://10.0.0.1:6443 prod/api',
        mechanism: { type: 'onerror', handled: false },
        stacktrace: {
          frames: [
            {
              filename: MAIN,
              abs_path: MAIN,
              function: 'Xe',
              lineno: 1,
              colno: 20,
              vars: { context: 'prod' },
            },
            {
              filename: WINDOWS_MAIN,
              abs_path: WINDOWS_MAIN,
              lineno: 3,
              colno: 4,
            },
            { filename: '[native code]', function: 'forEach' },
            {
              filename: 'https://example.com/evil.js',
              abs_path: 'https://example.com/evil.js',
              lineno: 9,
              colno: 9,
            },
          ],
        },
      },
    ],
  },
  ...overrides,
})

describe('scrub', () => {
  it('keeps the error type and bundle locations, nothing else', () => {
    expect(scrub(event())).toEqual({
      type: undefined,
      event_id: 'e1',
      timestamp: 1,
      level: 'error',
      platform: 'javascript',
      sdk: undefined,
      release: 'kftray@0.30.0',
      environment: 'production',
      tags: { app: 'kftray', surface: 'webview', run_id: 'r1' },
      debug_meta: {
        images: [
          {
            type: 'sourcemap',
            code_file: MAIN,
            debug_id: 'aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee',
          },
        ],
      },
      exception: {
        values: [
          {
            type: 'Error',
            mechanism: { type: 'onerror', handled: false },
            stacktrace: {
              frames: [
                {
                  abs_path: MAIN,
                  filename: MAIN,
                  lineno: 1,
                  colno: 20,
                  function: 'Xe',
                  in_app: true,
                },
                {
                  abs_path: WINDOWS_MAIN,
                  filename: WINDOWS_MAIN,
                  lineno: 3,
                  colno: 4,
                  function: undefined,
                  in_app: true,
                },
              ],
            },
          },
        ],
      },
    })
  })

  it('never carries the message, even through an error chain', () => {
    const scrubbed = scrub(
      event({
        exception: {
          values: [
            {
              type: 'Error',
              value: 'outer prod/api',
              mechanism: { type: 'chained', handled: true },
            },
            {
              type: 'TypeError',
              value: 'inner https://10.0.0.1',
              mechanism: { type: 'onerror', handled: false },
            },
          ],
        },
      }),
    )
    const json = JSON.stringify(scrubbed)

    expect(json).not.toContain('prod/api')
    expect(json).not.toContain('10.0.0.1')
    expect(scrubbed?.exception?.values?.map(e => e.type)).toEqual([
      'Error',
      'TypeError',
    ])
  })

  it('drops rejections the SDK synthesised from a non-Error value', () => {
    expect(
      scrub(
        event({
          exception: {
            values: [
              {
                type: 'UnhandledRejection',
                value:
                  'Non-Error promise rejection captured with value: Failed to forward context prod',
                mechanism: { type: 'onunhandledrejection', synthetic: true },
              },
            ],
          },
        }),
      ),
    ).toBeNull()
  })

  it('drops events without an exception', () => {
    expect(scrub(event({ exception: undefined }))).toBeNull()
  })
})

// Each webview runs its own copy of this module. These tests play the logs
// window: it never calls setCrashReportsConsent itself, so the only way a
// toggle made in the main window reaches it is the Rust broadcast.
const tauri = vi.hoisted(() => ({
  context: {
    dsn: null as string | null,
    enabled: true,
    performance_enabled: false,
  },
  // Keyed by event name; listeners are registered at init and cleared per
  // test, so a Map rather than a static record.
  listeners: new Map<string, Array<(event: { payload: boolean }) => void>>(),
  // Settled when the module asks for the context; the test then decides
  // when the answer arrives, so a broadcast can be timed against it.
  requested: Promise.withResolvers<void>(),
  releaseContext: vi.fn() as () => void,
}))

vi.mock('@tauri-apps/api/core', () => ({
  invoke: (command: string) => {
    if (command !== 'get_telemetry_context') {
      return Promise.resolve()
    }
    const { promise, resolve } = Promise.withResolvers<unknown>()
    tauri.releaseContext = () =>
      resolve({
        ...tauri.context,
        release: 'kftray@test',
        target: 'x86_64-unknown-linux-gnu',
        run_id: 'r',
      })
    tauri.requested.resolve()
    return promise
  },
}))

vi.mock('@tauri-apps/api/event', () => ({
  listen: (name: string, handler: (event: { payload: boolean }) => void) => {
    tauri.listeners.set(name, [...(tauri.listeners.get(name) ?? []), handler])
    return Promise.resolve(vi.fn())
  },
}))

const CRASH_EVENT = 'telemetry-enabled-changed'
const PERFORMANCE_EVENT = 'performance-enabled-changed'

const sentry = vi.hoisted(() => ({
  beforeSend: undefined as
    | ((event: ErrorEvent) => ErrorEvent | null)
    | undefined,
  beforeSendTransaction: undefined as
    | ((event: TransactionEvent) => TransactionEvent | null)
    | undefined,
  tracesSampler: undefined as (() => number) | undefined,
}))

vi.mock('@sentry/browser', () => ({
  init: (options: {
    beforeSend: typeof sentry.beforeSend
    beforeSendTransaction: typeof sentry.beforeSendTransaction
    tracesSampler: typeof sentry.tracesSampler
  }) => {
    sentry.beforeSend = options.beforeSend
    sentry.beforeSendTransaction = options.beforeSendTransaction
    sentry.tracesSampler = options.tracesSampler
  },
  globalHandlersIntegration: () => ({}),
  browserApiErrorsIntegration: () => ({}),
  linkedErrorsIntegration: () => ({}),
  dedupeIntegration: () => ({}),
}))

describe('consent in a window that did not change it', () => {
  beforeEach(() => {
    tauri.context = {
      dsn: 'https://k@example.invalid/1',
      enabled: true,
      performance_enabled: false,
    }
    tauri.listeners.clear()
    tauri.requested = Promise.withResolvers<void>()
    sentry.beforeSend = undefined
    sentry.beforeSendTransaction = undefined
    sentry.tracesSampler = undefined
  })

  it('stops reporting when the main window turns the switch off', async () => {
    const started = initCrashReporting()
    await tauri.requested.promise
    tauri.releaseContext()
    await started
    expect(sentry.beforeSend?.(event())).not.toBeNull()

    for (const listener of tauri.listeners.get(CRASH_EVENT) ?? []) {
      listener({ payload: false })
    }

    expect(sentry.beforeSend?.(event())).toBeNull()
  })

  it('keeps a broadcast that lands before the initial context', async () => {
    const started = initCrashReporting()
    await tauri.requested.promise
    // The listeners were registered before the context was requested...
    expect(tauri.listeners.get(CRASH_EVENT)).toHaveLength(1)
    for (const listener of tauri.listeners.get(CRASH_EVENT) ?? []) {
      listener({ payload: false })
    }
    // ...so the stale `enabled: true` in the context must not win.
    tauri.releaseContext()
    await started

    expect(sentry.beforeSend?.(event())).toBeNull()
  })

  it('never starts the SDK when the process is not allowed to report', async () => {
    tauri.context.dsn = null
    const started = initCrashReporting()
    await tauri.requested.promise
    tauri.releaseContext()
    await started

    expect(sentry.beforeSend).toBeUndefined()
  })

  it('samples invoke spans only while performance data is on', async () => {
    const started = initCrashReporting()
    await tauri.requested.promise
    tauri.releaseContext()
    await started
    expect(sentry.tracesSampler?.()).toBe(0)

    for (const listener of tauri.listeners.get(PERFORMANCE_EVENT) ?? []) {
      listener({ payload: true })
    }
    expect(sentry.tracesSampler?.()).toBe(1)
    expect(sentry.beforeSendTransaction?.(transaction())).not.toBeNull()

    for (const listener of tauri.listeners.get(PERFORMANCE_EVENT) ?? []) {
      listener({ payload: false })
    }
    expect(sentry.tracesSampler?.()).toBe(0)
    expect(sentry.beforeSendTransaction?.(transaction())).toBeNull()
  })
})

const transaction = (): TransactionEvent => ({
  type: 'transaction',
  event_id: 't1',
  transaction: 'get_configs',
  start_timestamp: 1,
  timestamp: 2,
  platform: 'javascript',
  release: 'kftray@0.30.0',
  environment: 'production',
  tags: { app: 'kftray', surface: 'webview', run_id: 'r1' },
  contexts: {
    trace: { trace_id: 't', span_id: 's', op: 'tauri.invoke', status: 'ok' },
    device: { name: 'laptop' },
  },
  request: {
    url: 'tauri://localhost/index.html',
    headers: { 'User-Agent': 'Mozilla/5.0 WebKit' },
  },
  breadcrumbs: [{ category: 'console', message: 'forwarding prod/api' }],
  extra: { args: { context: 'prod' } },
  spans: [
    {
      span_id: 'c',
      parent_span_id: 's',
      trace_id: 't',
      op: 'child',
      description: 'step',
      status: 'ok',
      start_timestamp: 1,
      timestamp: 1.5,
      data: { namespace: 'prod' },
    },
  ],
})

describe('scrubTransaction', () => {
  it('keeps timing and trace only', () => {
    expect(scrubTransaction(transaction())).toEqual({
      type: 'transaction',
      event_id: 't1',
      transaction: 'get_configs',
      start_timestamp: 1,
      timestamp: 2,
      platform: 'javascript',
      sdk: undefined,
      release: 'kftray@0.30.0',
      environment: 'production',
      tags: { app: 'kftray', surface: 'webview', run_id: 'r1' },
      contexts: {
        trace: {
          trace_id: 't',
          span_id: 's',
          op: 'tauri.invoke',
          status: 'ok',
        },
      },
      spans: [
        {
          span_id: 'c',
          parent_span_id: 's',
          trace_id: 't',
          op: 'child',
          description: 'step',
          status: 'ok',
          start_timestamp: 1,
          timestamp: 1.5,
          data: {},
        },
      ],
    })
  })
})
