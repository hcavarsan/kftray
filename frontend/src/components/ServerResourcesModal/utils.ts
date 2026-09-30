import type { ContextTarget } from './types'

export const CONTEXT_TIMEOUT_MS = 8000
export const RESOURCES_KEY = 'server-resources'

export const withTimeout = <T>(promise: Promise<T>, ms: number): Promise<T> => {
  let timer = 0

  return Promise.race([
    promise,
    new Promise<T>((_, reject) => {
      timer = window.setTimeout(() => reject(new Error('Timeout')), ms)
    }),
  ]).finally(() => clearTimeout(timer))
}

export const targetLabel = ({ context, kubeconfig }: ContextTarget): string =>
  kubeconfig ? `${context} (${kubeconfig.split('/').pop()})` : context
