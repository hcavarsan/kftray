import { useCallback, useEffect, useState } from 'react'

import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'

interface Shortcut {
  id: number
  name: string
  shortcut_key: string
  action_type: string
  action_data?: string
  config_id?: number
  enabled: boolean
}

interface PlatformStatus {
  platform: string
  is_wayland: boolean
  has_input_permissions: boolean
  needs_permission_fix: boolean
  current_implementation: string
  can_fix_permissions: boolean
}

interface GlobalShortcutHook {
  shortcuts: Shortcut[]
  platformStatus: PlatformStatus | null
  isFixingPermissions: boolean
  createShortcut: (
    name: string,
    shortcutKey: string,
    actionType: string,
  ) => Promise<number | null>
  updateShortcut: (
    id: number,
    name: string,
    shortcutKey: string,
    actionType: string,
  ) => Promise<boolean>
  deleteShortcut: (id: number) => Promise<boolean>
  validateShortcut: (shortcutKey: string) => Promise<boolean>
  normalizeShortcut: (shortcutKey: string) => Promise<string | null>
  refreshShortcuts: () => Promise<void>
  checkPlatformStatus: () => Promise<void>
  tryFixPermissions: () => Promise<boolean>
}

export const useGlobalShortcuts = (): GlobalShortcutHook => {
  const [shortcuts, setShortcuts] = useState<Shortcut[]>([])
  const [platformStatus, setPlatformStatus] = useState<PlatformStatus | null>(
    null,
  )
  const [isFixingPermissions, setIsFixingPermissions] = useState(false)

  const refreshShortcuts = useCallback(async () => {
    try {
      const allShortcuts = await invoke<Shortcut[]>('get_shortcuts')

      setShortcuts(allShortcuts)
    } catch (error) {
      console.error('Failed to load shortcuts:', error)
      setShortcuts([])
    }
  }, [])

  const checkPlatformStatus = useCallback(async () => {
    try {
      const status = await invoke<PlatformStatus>('get_platform_status')

      setPlatformStatus(status)
    } catch (error) {
      console.error('Failed to check platform status:', error)
      setPlatformStatus(null)
    }
  }, [])

  const tryFixPermissions = useCallback(async (): Promise<boolean> => {
    try {
      setIsFixingPermissions(true)
      const result = await invoke<string>('try_fix_platform_permissions')

      console.log('Permission fix result:', result)

      // Re-check platform status after fix attempt
      setTimeout(() => {
        checkPlatformStatus()
      }, 1000)

      return true
    } catch (error) {
      console.error('Failed to fix permissions:', error)

      return false
    } finally {
      setIsFixingPermissions(false)
    }
  }, [checkPlatformStatus])

  const createShortcut = useCallback(
    async (
      name: string,
      shortcutKey: string,
      actionType: string,
    ): Promise<number | null> => {
      try {
        const id = await invoke<number>('create_shortcut', {
          request: {
            name,
            shortcut_key: shortcutKey,
            action_type: actionType,
            enabled: true,
          },
        })

        await refreshShortcuts()

        return id
      } catch (error) {
        console.error(`Failed to create shortcut ${name}:`, error)

        return null
      }
    },
    [refreshShortcuts],
  )

  const updateShortcut = useCallback(
    async (
      id: number,
      name: string,
      shortcutKey: string,
      actionType: string,
    ): Promise<boolean> => {
      try {
        await invoke('update_shortcut', {
          id,
          request: {
            name,
            shortcut_key: shortcutKey,
            action_type: actionType,
            enabled: true,
          },
        })
        await refreshShortcuts()

        return true
      } catch (error) {
        console.error(`Failed to update shortcut ${id}:`, error)

        return false
      }
    },
    [refreshShortcuts],
  )

  const deleteShortcut = useCallback(
    async (id: number): Promise<boolean> => {
      try {
        await invoke('delete_shortcut', { id })
        await refreshShortcuts()

        return true
      } catch (error) {
        console.error(`Failed to delete shortcut ${id}:`, error)

        return false
      }
    },
    [refreshShortcuts],
  )

  const validateShortcut = useCallback(
    async (shortcutKey: string): Promise<boolean> => {
      try {
        return await invoke<boolean>('validate_shortcut_key', {
          shortcutKey,
        })
      } catch (error) {
        console.error('Failed to validate shortcut:', error)

        return false
      }
    },
    [],
  )

  const normalizeShortcut = useCallback(
    async (shortcutKey: string): Promise<string | null> => {
      try {
        return await invoke<string>('normalize_shortcut_key', {
          shortcutStr: shortcutKey,
        })
      } catch (error) {
        console.error('Failed to normalize shortcut:', error)

        return null
      }
    },
    [],
  )

  useEffect(() => {
    const setupEventListeners = async () => {
      // Listen for platform status updates
      const unlistenPlatformStatus = await listen<PlatformStatus>(
        'platform-status-update',
        event => {
          console.log('Platform status update:', event.payload)
          setPlatformStatus(event.payload)
        },
      )

      // Listen for permission fix success
      const unlistenFixSuccess = await listen<string>(
        'permission-fix-success',
        event => {
          console.log('Permission fix successful:', event.payload)
          // Show user notification about needing to logout/login
          if (typeof window !== 'undefined') {
            // You can integrate with your toast/notification system here
            alert(
              `Success: ${event.payload}\n\nPlease logout and login again for changes to take effect.`,
            )
          }
        },
      )

      // Listen for permission fix errors
      const unlistenFixError = await listen<string>(
        'permission-fix-error',
        event => {
          console.error('Permission fix failed:', event.payload)
          // Show user notification about failure
          if (typeof window !== 'undefined') {
            alert(`Permission fix failed: ${event.payload}`)
          }
        },
      )

      // Listen for permission fix attempts
      const unlistenFixAttempt = await listen(
        'permission-fix-attempted',
        event => {
          console.log('Permission fix attempted:', event.payload)
        },
      )

      return () => {
        unlistenPlatformStatus()
        unlistenFixSuccess()
        unlistenFixError()
        unlistenFixAttempt()
      }
    }

    setupEventListeners()
  }, [])

  useEffect(() => {
    refreshShortcuts()
    checkPlatformStatus()
  }, [refreshShortcuts, checkPlatformStatus])

  return {
    shortcuts,
    platformStatus,
    isFixingPermissions,
    createShortcut,
    updateShortcut,
    deleteShortcut,
    validateShortcut,
    normalizeShortcut,
    refreshShortcuts,
    checkPlatformStatus,
    tryFixPermissions,
  }
}

export type { PlatformStatus, Shortcut }
