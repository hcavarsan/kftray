export interface HelperAddress {
  service: string
  address: string
}

export interface HelperHostEntry {
  id: string
  ip: string
  hostname: string
}

/** Mirrors `HelperStatus` in `crates/kftray-tauri/src/commands/helper.rs`. */
export interface HelperStatus {
  running: boolean
  error: string | null
  /** `null` when a running helper could not be asked, not when it holds none. */
  addresses: HelperAddress[] | null
  host_entries: HelperHostEntry[] | null
}
