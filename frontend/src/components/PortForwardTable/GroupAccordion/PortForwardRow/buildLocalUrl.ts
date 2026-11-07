import type { Config } from '@/types'

export function buildLocalUrl(config: Config): string {
  if (config.workload_type === 'expose' && config.exposure_type === 'public') {
    const protocol = config.cert_manager_enabled ? 'https' : 'http'

    return `${protocol}://${config.alias}`
  }

  const host =
    config.workload_type === 'expose'
      ? config.local_address || '127.0.0.1'
      : config.domain_enabled
        ? config.alias
        : config.local_address || 'localhost'
  const urlHost =
    host?.includes(':') && !host.startsWith('[') ? `[${host}]` : host

  return `http://${urlHost}:${config.local_port}`
}
