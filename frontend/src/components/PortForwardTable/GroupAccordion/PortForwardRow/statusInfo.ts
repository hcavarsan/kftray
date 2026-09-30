import type { Config, PendingConfigAction } from '@/types'

export interface StatusInfo {
  color: string
  status: string
  description: string
}

export function getStatusInfo(
  config: Config,
  pendingAction: PendingConfigAction | null,
  activePod: string | null,
): StatusInfo {
  if (pendingAction?.timedOut) {
    return {
      color: 'status.unresponsive',
      status: 'Unresponsive',
      description:
        pendingAction.action === 'starting'
          ? 'Start is taking longer than expected...'
          : pendingAction.action === 'stopping'
            ? 'Stop is taking longer than expected...'
            : 'This action is taking longer than expected...',
    }
  }

  if (pendingAction?.action === 'starting') {
    return {
      color: 'app.accent',
      status: 'Starting',
      description:
        config.workload_type === 'expose'
          ? 'Expose tunnel is starting...'
          : 'Port forward is starting...',
    }
  }

  if (pendingAction?.action === 'stopping') {
    return {
      color: 'app.accent',
      status: 'Stopping',
      description:
        config.workload_type === 'expose'
          ? 'Expose tunnel is stopping...'
          : 'Port forward is stopping...',
    }
  }

  if (
    pendingAction?.action === 'saving' ||
    pendingAction?.action === 'deleting'
  ) {
    return {
      color: 'status.busy',
      status: 'Busy',
      description:
        pendingAction.action === 'saving'
          ? 'Saving configuration...'
          : 'Deleting configuration...',
    }
  }

  if (config.is_running) {
    if (activePod?.includes('pending-rollout')) {
      return {
        color: 'status.rollout',
        status: 'Rollout',
        description: 'Pod rollout in progress',
      }
    }

    if (config.workload_type === 'expose') {
      return {
        color: 'app.accent',
        status: 'Running',
        description: activePod
          ? `Tunnel active via ${activePod}`
          : 'Expose tunnel is active',
      }
    }

    return {
      color: 'app.accent',
      status: activePod ? 'Running' : 'Pending',
      description: activePod
        ? `Connected to ${activePod}`
        : 'Waiting for healthy pod...',
    }
  }

  return {
    color: 'status.stopped',
    status: 'Stopped',
    description: 'Port forward is stopped',
  }
}

export interface ConfigDetail {
  label: string
  value: string
}

export function getConfigDetails(
  config: Config,
  activePod: string | null,
): ConfigDetail[] {
  const details: ConfigDetail[] = [
    { label: 'Alias', value: config.alias ?? '' },
    { label: 'Workload', value: config.workload_type },
  ]

  if (config.workload_type === 'expose') {
    details.push({
      label: 'Exposure',
      value:
        config.exposure_type === 'public'
          ? 'Public (Internet)'
          : 'Cluster Only',
    })

    if (config.exposure_type === 'public') {
      const protocol = config.cert_manager_enabled ? 'https' : 'http'

      details.push({ label: 'URL', value: `${protocol}://${config.alias}` })
      details.push({
        label: 'TLS/SSL',
        value: config.cert_manager_enabled ? 'Enabled' : 'Disabled',
      })
      if (config.cert_manager_enabled) {
        details.push({
          label: 'Cert Issuer',
          value: config.cert_issuer || 'default',
        })
      }
    } else {
      details.push({
        label: 'URL',
        value: `http://${config.alias}.${config.namespace ?? ''}.svc.cluster.local:${config.local_port}`,
      })
      details.push({ label: 'Service Name', value: config.alias })
    }
    details.push({ label: 'Local Port', value: String(config.local_port) })
  } else {
    details.push({
      label: 'Service',
      value: config.workload_type === 'service' ? config.service : '',
    })
    details.push({
      label: 'Target Port',
      value: String(config.remote_port ?? ''),
    })
    details.push({ label: 'Protocol', value: config.protocol ?? '' })
  }

  details.push({ label: 'Context', value: config.context ?? '' })
  details.push({ label: 'Namespace', value: config.namespace ?? '' })

  const tagsLabel = Object.entries(config.tags ?? {})
    .map(([key, value]) => (value ? `${key}=${value}` : key))
    .join(', ')

  if (tagsLabel) {
    details.push({ label: 'Tags', value: tagsLabel })
  }

  if (activePod) {
    details.push({ label: 'Active Pod', value: activePod })
  }

  return details
}

export function formatConfigDetails(
  status: StatusInfo,
  details: ConfigDetail[],
): string {
  const lines = [
    `Status: ${status.status}`,
    ...details.map(detail => `${detail.label}: ${detail.value}`),
  ]

  return `${lines.join('\n')}\n`
}
