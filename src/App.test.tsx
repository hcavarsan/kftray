import { describe, expect, it, vi } from 'vitest'

import { render, screen } from '@testing-library/react'

import App from './App'

import '@testing-library/jest-dom'

const mockConfigsResponse = [
  {
    id: 1,
    service: 'kubetest',
    context: 'kubetest',
    local_port: '1010',
    remote_port: '1010',
    namespace: 'default',
    isRunning: false,
  },
]

vi.mock('@tauri-apps/api/tauri', () => ({
  invoke: vi.fn(cmd => {
    if (cmd === 'get_configs') {
      return Promise.resolve(mockConfigsResponse)
    }
    
    return Promise.resolve()
  }),
}))

describe('App', () => {
  it('renders without crashing', async () => {
    render(<App />)
    expect(await screen.findByText('Start Forward')).toBeInTheDocument()
    expect(await screen.findByText('Stop Forward')).toBeInTheDocument()
  })
})
