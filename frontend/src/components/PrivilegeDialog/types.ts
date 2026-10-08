/** Mirrors `PrivilegedResource` in `crates/kftray-portforward/src/privileges.rs`. */
export type PrivilegedResource = 'hosts_file' | 'loopback_address'

/** Mirrors `PrivilegeNeed` in `crates/kftray-portforward/src/privileges.rs`. */
export interface PrivilegeNeed {
  config_id: number
  resource: PrivilegedResource
  detail: string
}
