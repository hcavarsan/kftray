export interface Config {
  id: number
  service: string
  namespace: string
  local_port: number
  local_address: string
  auto_loopback_address: boolean
  domain_enabled: boolean
  remote_port?: number
  context: string
  alias: string
  remote_address: string
  workload_type: string
  target: string
  protocol: string
  kubeconfig: string
  is_running: boolean
  http_logs_enabled?: boolean
  exposure_type?: string
  cert_manager_enabled?: boolean
  cert_issuer?: string
  cert_issuer_kind?: string
  ingress_class?: string
  ingress_annotations?: string
  tags?: Record<string, string>
}

export interface ViewCondition {
  field: string
  values: string[]
}

export interface ConfigView {
  group_by: string | null
  filters: ViewCondition[]
}

export interface ConfigGroup {
  key: string | null
  label: string
  config_ids: number[]
}

export interface FacetValue {
  value: string
  count: number
}

export interface Facet {
  field: string
  count: number
  values: FacetValue[]
}

export interface ConfigViewResult {
  view: ConfigView
  groups: ConfigGroup[]
  facets: Facet[]
}

export interface ResolvedGroup {
  id: string
  label: string
  configs: Config[]
}

export type PortForwardAction = 'starting' | 'stopping' | 'saving' | 'deleting'

export type PortForwardToggleAction = 'starting' | 'stopping'

export interface PendingConfigAction {
  action: PortForwardAction
  token: number
  timedOut?: boolean
}

export interface PortForwardResponse {
  id: number | null
  service: string
  namespace: string
  local_port: number
  remote_port: number
  context: string
  stdout: string
  stderr: string
  status: number
  protocol: string
}

type AuthMethod = 'none' | 'system' | 'token'

export interface GitConfig {
  repoUrl: string
  configPaths: string[]
  authMethod: AuthMethod
  token?: string
  isPrivate?: boolean
  pollingInterval: number
  flush?: boolean
}

export interface GitSyncModalProps {
  isGitSyncModalOpen: boolean
  closeGitSyncModal: () => void
  credentialsSaved: boolean
  setCredentialsSaved: (value: boolean) => void
  setPollingInterval: (value: number) => void
  pollingInterval: number
  onSuccessfulSave?: () => void
}

export interface TableProps {
  configs: Config[]
  isInitiating: boolean
  isStopping: boolean
  pendingConfigActions: Map<number, PendingConfigAction>
  toggleConfigForward: (
    config: Config,
    action: PortForwardToggleAction,
  ) => Promise<void>
  initiatePortForwarding: (configs: Config[]) => Promise<void>
  startSelectedPortForwarding: () => Promise<void>
  stopSelectedPortForwarding: () => Promise<void>
  stopAllPortForwarding: () => Promise<void>
  abortStartOperation: () => void
  abortStopOperation: () => void
  deleteConfigs: (ids: number[]) => Promise<boolean>
  handleEditConfig: (id: number) => Promise<void>
  handleDuplicateConfig: (id: number) => Promise<void>
  selectedConfigs: Config[]
  setSelectedConfigs: React.Dispatch<React.SetStateAction<Config[]>>
  openSettingsModal: () => void
  openServerResourcesModal: () => void
}

export interface PortForwardRowProps {
  config: Config
  deleteConfigs: (ids: number[]) => Promise<boolean>
  handleEditConfig: (id: number) => Promise<void>
  handleDuplicateConfig: (id: number) => Promise<void>
  showContext?: boolean
  onSelectionChange: (id: number, isSelected: boolean) => void
  selected: boolean
  pendingAction: PendingConfigAction | null
  toggleConfigForward: (
    config: Config,
    action: PortForwardToggleAction,
  ) => Promise<void>
}

export interface SyncStatus {
  lastSyncTime: number | null
  pollingInterval: number
  isSuccessful: boolean
  isSyncing: boolean
}

export interface FooterProps {
  openModal: () => void
  openGitSyncModal: () => void
  handleExportConfigs: () => void
  handleImportConfigs: () => void
  credentialsSaved: boolean
  setCredentialsSaved: (value: boolean) => void
  isGitSyncModalOpen: boolean
  selectedConfigs: Config[]
  setPollingInterval: (value: number) => void
  pollingInterval: number
  syncStatus: SyncStatus
  onSyncComplete: () => void
  openShortcutModal: () => void
  setIsAutoImportModalOpen: (open: boolean) => void
  deleteConfigs: (ids: number[]) => Promise<boolean>
}

export interface SyncConfigsButtonProps {
  serviceName: string
  accountName: string
  onSyncFailure: (error: Error) => void
  credentialsSaved: boolean
  setCredentialsSaved: (value: boolean) => void
  isGitSyncModalOpen: boolean
  setPollingInterval: (value: number) => void
  pollingInterval: number
  syncStatus: SyncStatus
  onSyncComplete?: () => void
}

export interface KubeContext {
  name: string
  cluster?: string
  user?: string
}

export interface CustomConfigProps {
  isModalOpen: boolean
  closeModal: () => void
  newConfig: Config
  handleInputChange: (event: React.ChangeEvent<HTMLInputElement>) => void
  handleSaveConfig: (config: Config) => Promise<boolean>
  handleEditSubmit: (e: React.FormEvent) => Promise<void>
  isEdit: boolean
  cancelRef: React.RefObject<HTMLElement>
  setNewConfig: React.Dispatch<React.SetStateAction<Config>>
}

export interface HeaderProps {
  search: string
  setSearch: React.Dispatch<React.SetStateAction<string>>
  openSettingsModal: () => void
  openServerResourcesModal: () => void
}

export interface HeaderMenuProps {
  isSelectAllChecked: boolean
  setIsSelectAllChecked: React.Dispatch<React.SetStateAction<boolean>>
  configs: Config[]
  selectedConfigs: Config[]
  initiatePortForwarding: (configs: Config[]) => void
  startSelectedPortForwarding: () => void
  stopSelectedPortForwarding: () => void
  stopAllPortForwarding: () => void
  abortStartOperation: () => void
  abortStopOperation: () => void
  isInitiating: boolean
  isStopping: boolean
  toggleExpandAll: () => void
  expandedIndices: string[]
  groupCount: number
  view: ConfigView | null
  facets: Facet[]
  setView: (view: ConfigView) => void
  setSelectedConfigs: React.Dispatch<React.SetStateAction<Config[]>>
}

export interface BulkDeleteButtonProps {
  selectedConfigs: Config[]
  deleteConfigs: (ids: number[]) => Promise<boolean>
}

export interface GroupAccordionProps {
  group: ResolvedGroup
  selectedConfigs: Config[]
  deleteConfigs: (ids: number[]) => Promise<boolean>
  handleEditConfig: (id: number) => Promise<void>
  handleDuplicateConfig: (id: number) => Promise<void>
  handleSelectionChange: (id: number, isSelected: boolean) => void
  handleCheckboxChange: (group: ResolvedGroup, isChecked: boolean) => void
  pendingConfigActions: Map<number, PendingConfigAction>
  toggleConfigForward: (
    config: Config,
    action: PortForwardToggleAction,
  ) => Promise<void>
}

export interface AutoImportModalProps {
  isOpen: boolean
  onClose: () => void
}

export interface ServiceData {
  name: string
  port?: number
}

export interface StringOption {
  label: string
  value: string
}

export interface PortOption {
  label: string
  value: number
}

export interface ServerResource {
  resource_type: string
  name: string
  namespace: string
  config_id: string | null
  is_orphaned: boolean
  age: string
  status: string
}

export interface NamespaceGroup {
  namespace: string
  resources: ServerResource[]
}
