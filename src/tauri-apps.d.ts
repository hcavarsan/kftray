declare module '@tauri-apps/api/dialog' {
  export function save(options: {
    defaultPath: string
    filters?: Array<{ name: string; extensions: string[] }>
  }): Promise<string | null>

  export function open(options: {
    filters: Array<{ name: string; extensions: string[] }>
    multiple: boolean
  }): Promise<string[]>
}

declare module '@tauri-apps/api/fs' {
  export function writeTextFile(path: string, content: string): Promise<void>
  export function readTextFile(path: string): Promise<string>
}

declare module '@tauri-apps/api/notification' {
  export function sendNotification(notification: {
    title: string
    body: string
    icon: string
  }): Promise<void>
}

declare module '@tauri-apps/api/tauri' {
  export function invoke<T = unknown>(
    command: string,
    payload?: unknown,
  ): Promise<T>
}
