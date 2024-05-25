import { create } from 'zustand'

import { listen } from '@tauri-apps/api/event'
import { invoke } from '@tauri-apps/api/tauri'

import { Status } from '../types'

interface ConfigState {
  configs: Status[]
  isInitiating: boolean
  isStopping: boolean
  isPortForwarding: boolean
  configState: { [key: number]: { config: Status; running: boolean } }
  setConfigs: (configs: Status[]) => void
  setIsInitiating: (isInitiating: boolean) => void
  setIsStopping: (isStopping: boolean) => void
  setIsPortForwarding: (isPortForwarding: boolean) => void
  syncConfigsAndUpdateState: () => Promise<void>
  updateConfigRunningState: (id: number, isRunning: boolean) => void
}

const useConfigStore = create<ConfigState>(set => ({
  configs: [],
  isInitiating: false,
  isStopping: false,
  isPortForwarding: false,
  configState: {},
  setConfigs: configs => set({ configs }),
  setIsInitiating: isInitiating => set({ isInitiating }),
  setIsStopping: isStopping => set({ isStopping }),
  setIsPortForwarding: isPortForwarding => set({ isPortForwarding }),
  syncConfigsAndUpdateState: async () => {
    try {
      const updatedConfigs = await invoke<Status[]>('get_configs')

      set({ configs: updatedConfigs })
      const updatedConfigState = updatedConfigs.reduce(
        (acc, config) => {
          acc[config.id] = { config, running: config.isRunning }

          return acc
        },
        {} as { [key: number]: { config: Status; running: boolean } },
      )

      set({ configState: updatedConfigState })
    } catch (error) {
      console.error('Error syncing configs:', error)
    }
  },
  updateConfigRunningState: (id, isRunning) => {
    set(state => ({
      configState: {
        ...state.configState,
        [id]: { ...state.configState[id], running: isRunning },
      },
    }))
  },
}))

// Initialize the store with data from the backend
const initializeStore = async () => {
  const configs = await invoke<Status[]>('get_configs')
  const initialConfigState = configs.reduce(
    (acc, config) => {
      acc[config.id] = { config, running: config.isRunning }

      return acc
    },
    {} as { [key: number]: { config: Status; running: boolean } },
  )

  useConfigStore.setState({
    configs,
    configState: initialConfigState,
  })

  listen<{ id: number; config: Status; running: boolean }>(
    'config_state_changed',
    event => {
      const { id, config, running } = event.payload

      useConfigStore.setState(state => ({
        configState: {
          ...state.configState,
          [id]: { config, running },
        },
      }))
    },
  )
}

initializeStore()

export default useConfigStore
