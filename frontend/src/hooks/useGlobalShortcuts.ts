import { useCallback, useEffect, useState } from 'react'

import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

interface ShortcutAction {
  id: string
  name: string
  description: string
  defaultShortcut: string
  action: string
}

interface GlobalShortcutHook {
  shortcuts: Record<string, string>
  hasLinuxPermissions: boolean
  isLinux: boolean
  registerShortcut: (
    id: string,
    shortcut: string,
    action: string,
  ) => Promise<boolean>
  unregisterShortcut: (id: string) => Promise<boolean>
  testShortcutFormat: (shortcut: string) => Promise<boolean>
  refreshShortcuts: () => Promise<void>
  tryFixLinuxPermissions: () => Promise<boolean>
}

const DEFAULT_SHORTCUTS: ShortcutAction[] = [
  {
    id: 'toggle_window',
    name: 'Toggle Window',
    description: 'Show/hide the main application window',
    defaultShortcut: 'ctrl+shift+k',
    action: 'toggle_window',
  },
]

export const useGlobalShortcuts = (): GlobalShortcutHook => {
  const [shortcuts, setShortcuts] = useState<Record<string, string>>({})
  const [hasLinuxPermissions, setHasLinuxPermissions] = useState(true)
  const [isLinux, setIsLinux] = useState(false)

  useEffect(() => {
    const checkPlatformAndPermissions = async () => {
      try {
        const [isLinux, hasPermissions] = await invoke<[boolean, boolean]>(
          'check_linux_permissions',
        )

        console.log(
          `Backend detection - isLinux: ${isLinux}, hasPermissions: ${hasPermissions}`,
        )

        setIsLinux(isLinux)
        setHasLinuxPermissions(hasPermissions)
      } catch (error) {
        console.error('Error checking platform/permissions:', error)
        setIsLinux(false)
        setHasLinuxPermissions(true)
      }
    }

    checkPlatformAndPermissions()
  }, [])

  const refreshShortcuts = useCallback(async () => {
    try {
      const registered = await invoke<Record<string, string>>(
        'get_registered_shortcuts',
      )

      setShortcuts(registered)
    } catch (error) {
      console.error('Failed to load shortcuts:', error)
      setShortcuts({})
    }
  }, [])

  const registerShortcut = useCallback(
    async (id: string, shortcut: string, action: string): Promise<boolean> => {
      try {
        await invoke('register_global_shortcut', {
          shortcutId: id,
          shortcutStr: shortcut,
          action,
        })
        await refreshShortcuts()

        return true
      } catch (error) {
        console.error(`Failed to register shortcut ${id}:`, error)

        return false
      }
    },
    [refreshShortcuts],
  )

  const unregisterShortcut = useCallback(
    async (id: string): Promise<boolean> => {
      try {
        await invoke('unregister_global_shortcut', { shortcutId: id })
        await refreshShortcuts()

        return true
      } catch (error) {
        console.error(`Failed to unregister shortcut ${id}:`, error)

        return false
      }
    },
    [refreshShortcuts],
  )

  const testShortcutFormat = useCallback(
    async (shortcut: string): Promise<boolean> => {
      try {
        return await invoke<boolean>('test_shortcut_format', {
          shortcutStr: shortcut,
        })
      } catch (error) {
        console.error('Failed to test shortcut format:', error)

        return false
      }
    },
    [],
  )

  useEffect(() => {
    const setupEventListeners = async () => {
      const unlisten = await listen('shortcut-triggered', event => {
        const [shortcutId, action] = (event.payload as string).split(':')

        console.log(`Global shortcut triggered: ${shortcutId} -> ${action}`)
        handleShortcutAction(action)
      })

      const toggleWindowUnlisten = await listen(
        'shortcut-triggered:toggle_window',
        event => {
          const action = event.payload as string

          console.log(`Toggle window shortcut triggered with action: ${action}`)
          handleShortcutAction(action)
        },
      )

      return () => {
        unlisten()
        toggleWindowUnlisten()
      }
    }

    setupEventListeners()
  }, [])

  const handleShortcutAction = (action: string) => {
    if (action === 'toggle_window') {
      console.log('Toggle window action triggered')
    } else {
      console.log(`Unknown shortcut action: ${action}`)
    }
  }

  const tryFixLinuxPermissions = useCallback(async (): Promise<boolean> => {
    try {
      const result = await invoke<boolean>('try_fix_linux_permissions')

      if (result) {
        const [, hasPermissions] = await invoke<[boolean, boolean]>(
          'check_linux_permissions',
        )

        setHasLinuxPermissions(hasPermissions)
      }

      return result
    } catch (error) {
      console.error('Failed to fix Linux permissions:', error)

      return false
    }
  }, [])

  useEffect(() => {
    refreshShortcuts()
  }, [refreshShortcuts])

  return {
    shortcuts,
    hasLinuxPermissions,
    isLinux,
    registerShortcut,
    unregisterShortcut,
    testShortcutFormat,
    refreshShortcuts,
    tryFixLinuxPermissions,
  }
}

export { DEFAULT_SHORTCUTS }
export type { ShortcutAction }
