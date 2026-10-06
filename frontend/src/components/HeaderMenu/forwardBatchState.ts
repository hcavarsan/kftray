import type { Config } from '@/types'

export interface ForwardBatchState {
  selected: Config[]
  startSelected: boolean
  stopSelected: boolean
  startDisabled: boolean
  stopDisabled: boolean
}

export function forwardBatchState(
  configs: Config[],
  selectedConfigs: Config[],
): ForwardBatchState {
  const selectedIds = new Set(selectedConfigs.map(config => config.id))
  const selected = configs.filter(config => selectedIds.has(config.id))
  const startSelected = selected.some(config => !config.is_running)
  const stopSelected = selected.some(config => config.is_running)

  if (selected.length > 0) {
    return {
      selected,
      startSelected,
      stopSelected,
      startDisabled: !startSelected,
      stopDisabled: !stopSelected,
    }
  }

  return {
    selected,
    startSelected,
    stopSelected,
    startDisabled: configs.every(config => config.is_running),
    stopDisabled: configs.every(config => !config.is_running),
  }
}
