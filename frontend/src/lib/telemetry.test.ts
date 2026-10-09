import { describe, expect, it } from 'vitest'

import type { ErrorEvent } from '@sentry/browser'

import { scrub } from './telemetry'

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
