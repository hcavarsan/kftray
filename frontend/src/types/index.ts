import { RefObject } from 'react'

export interface Status {
  id: number
  service: string
  context: string
  local_port: number
  isRunning: boolean
  namespace: string
  remote_port: number
  local_address: string
  domain_enabled: boolean
  workload_type: string
  protocol: string
  alias: string
  remote_address: string
  cancelRef?: RefObject<HTMLButtonElement>
  kubeconfig?: string
}

export interface Config {
  id: number
  service: string
  namespace: string
  local_port: number
  local_address: string
  domain_enabled: boolean
  remote_port: number
  context: string
  alias: string
  remote_address: string
  workload_type: string
  protocol: string
  kubeconfig: string
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
  protocol: string
  remote_address: string
  stdout: string
  stderr: string
}

export interface GitConfig {
  repoUrl: string
  configPath: string
  isPrivate: boolean
  pollingInterval: number
  token: string
  flush: boolean
}

export interface ConfigProps {
  isModalOpen: boolean
  closeModal: () => void
  newConfig: Config
  handleInputChange: (event: React.ChangeEvent<HTMLInputElement>) => void
  handleSaveConfig: (config: Config) => Promise<void>
  handleEditSubmit: (e: React.FormEvent) => Promise<void>
  cancelRef: RefObject<HTMLElement>
  isEdit: boolean
}

export interface GitSyncModalProps {
  isGitSyncModalOpen: boolean
  closeGitSyncModal: () => void
  onSettingsSaved: () => void
  credentialsSaved: boolean
  setCredentialsSaved: React.Dispatch<React.SetStateAction<boolean>>
  setPollingInterval: React.Dispatch<React.SetStateAction<number>>
  pollingInterval: number
}

export interface TableProps {
  configs: Status[]
  isInitiating: boolean
  isStopping: boolean
  isPortForwarding: boolean
  initiatePortForwarding: (configs: Status[]) => Promise<void>
  stopPortForwarding: (configs: Status[]) => Promise<void>
  confirmDeleteConfig: () => void
  handleDeleteConfig: (id: number) => void
  handleEditConfig: (id: number) => void
  isAlertOpen: boolean
  setIsAlertOpen: (isOpen: boolean) => void
  updateConfigRunningState: (id: number, isRunning: boolean) => void
  selectedConfigs: Status[]
  setSelectedConfigs: React.Dispatch<React.SetStateAction<Status[]>>
  setIsInitiating: React.Dispatch<React.SetStateAction<boolean>>
}

export interface PortForwardRowProps {
  config: Status
  confirmDeleteConfig: () => void
  handleDeleteConfig: (id: number) => void
  handleEditConfig: (id: number) => void
  isAlertOpen: boolean
  setIsAlertOpen: (isOpen: boolean) => void
  updateConfigRunningState: (id: number, isRunning: boolean) => void
  showContext?: boolean
  onSelectionChange: (isSelected: boolean) => void
  updateSelectionState: (id: number, isRunning: boolean) => void
  selected: boolean
  isInitiating: boolean
  setIsInitiating: React.Dispatch<React.SetStateAction<boolean>>
  isStopping: boolean
}

export interface FooterProps {
  openModal: () => void
  openGitSyncModal: () => void
  handleExportConfigs: () => void
  handleImportConfigs: () => void
  credentialsSaved: boolean
  setCredentialsSaved: React.Dispatch<React.SetStateAction<boolean>>
  onConfigsSynced: () => void
  isGitSyncModalOpen: boolean
  selectedConfigs: Status[]
  setPollingInterval: React.Dispatch<React.SetStateAction<number>>
  pollingInterval: number
  setSelectedConfigs: React.Dispatch<React.SetStateAction<Status[]>>
  configs: Status[]
  setConfigs: React.Dispatch<React.SetStateAction<Status[]>>
}

export interface SyncConfigsButtonProps {
  serviceName: string
  accountName: string
  onConfigsSynced?: () => void
  onSyncFailure?: (error: Error) => void
  credentialsSaved: boolean
  setCredentialsSaved: React.Dispatch<React.SetStateAction<boolean>>
  isGitSyncModalOpen: boolean
  setPollingInterval: React.Dispatch<React.SetStateAction<number>>
  pollingInterval: number
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
  configData?: {
    context?: KubeContext[]
    namespace?: Namespace[]
    service?: Service[]
    port: number
    name?: string
    remote_port?: number
    ports?: Port[]
    kubeconfig?: string
  }
  setNewConfig: React.Dispatch<React.SetStateAction<Config>>
}

export type Option = { name: string; value: string | number; label: string }

export interface CustomToastProps {
  title: string
  description: string
  status: 'info' | 'warning' | 'success' | 'error'
}

export interface ShowToastParams {
  title: string
  description?: string
  status?: 'info' | 'warning' | 'success' | 'error'
  duration?: number
  isClosable?: boolean
  position?:
    | 'top'
    | 'top-right'
    | 'top-left'
    | 'bottom'
    | 'bottom-right'
    | 'bottom-left'
}

export interface ConfigsByContext {
  [key: string]: Status[]
}

export interface HeaderProps {
  search: string
  setSearch: React.Dispatch<React.SetStateAction<string>>
}

export interface HeaderMenuProps {
  isSelectAllChecked: boolean
  setIsSelectAllChecked: React.Dispatch<React.SetStateAction<boolean>>
  configs: Status[]
  selectedConfigs: Status[]
  initiatePortForwarding: (configs: Status[]) => void
  startSelectedPortForwarding: () => void
  stopAllPortForwarding: () => void
  isInitiating: boolean
  isStopping: boolean
  toggleExpandAll: () => void
  expandedIndices: number[]
  configsByContext: ConfigsByContext
  setSelectedConfigs: React.Dispatch<React.SetStateAction<Status[]>>
}

export interface BulkDeleteButtonProps {
  selectedConfigs: Status[]
  setSelectedConfigs: React.Dispatch<React.SetStateAction<Status[]>>
  configs: Status[]
  setConfigs: React.Dispatch<React.SetStateAction<Status[]>>
}

export interface ContextsAccordionProps {
  context: string
  contextConfigs: Status[]
  selectedConfigs: Status[]
  handleDeleteConfig: (id: number) => void
  confirmDeleteConfig: () => void
  handleEditConfig: (id: number) => void
  isAlertOpen: boolean
  setIsAlertOpen: (isOpen: boolean) => void
  updateConfigRunningState: (id: number, isRunning: boolean) => void
  handleSelectionChange: (config: Status, isSelected: boolean) => void
  updateSelectionState: (id: number, isRunning: boolean) => void
  selectedConfigsByContext: Record<string, boolean>
  handleCheckboxChange: (context: string, isChecked: boolean) => void
  isInitiating: boolean
  setIsInitiating: React.Dispatch<React.SetStateAction<boolean>>
  isStopping: boolean
}
