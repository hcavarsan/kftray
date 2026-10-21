import { visibleSelection } from '@/components/PortForwardTable/viewUtils'
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
  allConfigs: Config[],
): ForwardBatchState {
  const selected = visibleSelection(configs, selectedConfigs)
  const startSelected = selected.some(config => !config.is_running)
  const stopSelected = selected.some(
    config => config.is_running && !config.owned_elsewhere,
  )

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
    stopDisabled: !allConfigs.some(
      config => config.is_running && !config.owned_elsewhere,
    ),
  }
}
