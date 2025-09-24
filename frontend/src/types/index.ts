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
}

export interface Response {
  id: number
  service: string
  context: string
  local_port: number
  status: number
  namespace: string
  remote_port: number
  alias: string
  workload_type: string
  target: string
  protocol: string
  remote_address: string
  stdout: string
  stderr: string
}

export type AuthMethod = 'none' | 'system' | 'token'

export interface GitConfig {
  repoUrl: string
  configPath: string
  authMethod: AuthMethod
  token?: string
  isPrivate?: boolean
  pollingInterval: number
  flush?: boolean
}

export interface ConfigProps {
  isModalOpen: boolean
  closeModal: () => void
  newConfig: Config
  handleInputChange: (event: React.ChangeEvent<HTMLInputElement>) => void
  handleSaveConfig: (config: Config) => Promise<void>
  handleEditSubmit: (e: React.FormEvent) => Promise<void>
  cancelRef: React.RefObject<HTMLElement>
  isEdit: boolean
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
  initiatePortForwarding: (configs: Config[]) => Promise<void>
  startSelectedPortForwarding: () => Promise<void>
  stopSelectedPortForwarding: () => Promise<void>
  stopAllPortForwarding: () => Promise<void>
  confirmDeleteConfig: () => void
  handleDeleteConfig: (id: number) => void
  handleEditConfig: (id: number) => void
  isAlertOpen: boolean
  setIsAlertOpen: (open: boolean) => void
  selectedConfigs: Config[]
  setSelectedConfigs: React.Dispatch<React.SetStateAction<Config[]>>
  setIsInitiating: React.Dispatch<React.SetStateAction<boolean>>
  openSettingsModal: () => void
}

export interface PortForwardRowProps {
  config: Config
  confirmDeleteConfig: () => void
  handleDeleteConfig: (id: number) => void
  handleEditConfig: (id: number) => void
  isAlertOpen: boolean
  setIsAlertOpen: (open: boolean) => void
  showContext?: boolean
  onSelectionChange: (isSelected: boolean) => void
  selected: boolean
  _isInitiating: boolean
  setIsInitiating: React.Dispatch<React.SetStateAction<boolean>>
  isStopping: boolean
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
  setSelectedConfigs: (configs: Config[]) => void
  configs: Config[]
  syncStatus: SyncStatus
  onSyncComplete: () => void
  openShortcutModal: () => void
  setIsAutoImportModalOpen: (open: boolean) => void
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

export interface Namespace {
  namespace: string
  name: string
}

export interface Context {
  value: string
  label: string
}

export interface KubeContext {
  name: string
  cluster?: string
  user?: string
}

export interface Service {
  name: string
  service: string
}

export interface Port {
  remote_port: number
  name?: string
  port?: number
}

export interface CustomConfigProps extends ConfigProps {
  setNewConfig: React.Dispatch<React.SetStateAction<Config>>
}

export interface Option {
  name?: string
  value: string | number
  label: string
}

export interface ConfigsByContext {
  [key: string]: Config[]
}

export interface HeaderProps {
  search: string
  setSearch: React.Dispatch<React.SetStateAction<string>>
  openSettingsModal: () => void
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
  isInitiating: boolean
  isStopping: boolean
  toggleExpandAll: () => void
  expandedIndices: string[]
  configsByContext: ConfigsByContext
  setSelectedConfigs: React.Dispatch<React.SetStateAction<Config[]>>
}

export interface BulkDeleteButtonProps {
  selectedConfigs: Config[]
  setSelectedConfigs: (configs: Config[]) => void
  configs: Config[]
}

export interface ContextsAccordionProps {
  context: string
  contextConfigs: Config[]
  selectedConfigs: Config[]
  handleDeleteConfig: (id: number) => void
  confirmDeleteConfig: () => void
  handleEditConfig: (id: number) => void
  isAlertOpen: boolean
  setIsAlertOpen: (open: boolean) => void
  handleSelectionChange: (config: Config, isSelected: boolean) => void
  selectedConfigsByContext: Record<string, boolean>
  handleCheckboxChange: (context: string, isChecked: boolean) => void
  isInitiating: boolean
  setIsInitiating: React.Dispatch<React.SetStateAction<boolean>>
  isStopping: boolean
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
