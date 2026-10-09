import { invoke } from '@/lib/tauri'
import type { PortForwardResponse, StoredConfig } from '@/types'

function usesTcpForward(config: StoredConfig) {
  switch (config.workload_type) {
    case 'expose':
      return true
    case 'proxy':
      return false
    case 'service':
    case 'pod':
      return config.protocol === 'tcp'
  }
}

export async function startForward(config: StoredConfig) {
  const tcp = usesTcpForward(config)
  const responses = await invoke<PortForwardResponse[]>(
    tcp ? 'start_port_forward_tcp_cmd' : 'deploy_and_forward_pod_cmd',
    { configs: [config] },
  )
  const failure = responses.find(response => response.status !== 0)

  if (failure) {
    throw new Error(failure.stderr || 'Failed to start port forwarding.')
  }
}

export async function stopForward(config: StoredConfig) {
  await invoke('stop_port_forward_cmd', { configId: config.id.toString() })
}
