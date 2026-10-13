import { invoke } from '@/lib/tauri'
import type { Config, StoredConfig } from '@/types'

// Matches the busy-port start error built in
// crates/kftray-portforward/src/local_port.rs.
const FREE_PORT = /is already in use\. Port (\d+) is free\./

export function freePortFromError(message: string): number | undefined {
  const match = FREE_PORT.exec(message)

  return match ? Number(match[1]) : undefined
}

export async function moveToFreePort(
  config: Config,
  port: number,
): Promise<Config> {
  const stored = await invoke<StoredConfig>('get_config_cmd', { id: config.id })

  await invoke('update_config_cmd', { config: { ...stored, local_port: port } })

  return { ...config, local_port: port }
}
