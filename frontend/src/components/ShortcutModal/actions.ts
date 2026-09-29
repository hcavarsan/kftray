export const SHORTCUT_ACTIONS = [
  { actionType: 'toggle_window', name: 'Toggle Window', requiresConfig: false },
  {
    actionType: 'start_all_port_forward',
    name: 'Start All Port Forward',
    requiresConfig: false,
  },
  {
    actionType: 'stop_all_port_forward',
    name: 'Stop All Port Forward',
    requiresConfig: false,
  },
  {
    actionType: 'start_port_forward',
    name: 'Start Port Forward',
    requiresConfig: true,
  },
  {
    actionType: 'stop_port_forward',
    name: 'Stop Port Forward',
    requiresConfig: true,
  },
  {
    actionType: 'toggle_port_forward',
    name: 'Toggle Port Forward',
    requiresConfig: true,
  },
] as const

export type ShortcutActionType = (typeof SHORTCUT_ACTIONS)[number]['actionType']

export const findShortcutAction = (actionType: string) =>
  SHORTCUT_ACTIONS.find(action => action.actionType === actionType)

export function shortcutConfigIds(actionData: string | undefined): number[] {
  if (!actionData) {
    return []
  }
  try {
    const parsed: unknown = JSON.parse(actionData)

    if (
      parsed &&
      typeof parsed === 'object' &&
      'config_ids' in parsed &&
      Array.isArray(parsed.config_ids)
    ) {
      return parsed.config_ids.filter(
        (id): id is number => typeof id === 'number',
      )
    }
  } catch {
    return []
  }

  return []
}
