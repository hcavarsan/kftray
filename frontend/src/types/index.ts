import { RefObject } from 'react'

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
  credentialsSaved: boolean
  setCredentialsSaved: React.Dispatch<React.SetStateAction<boolean>>
  setPollingInterval: React.Dispatch<React.SetStateAction<number>>
  pollingInterval: number
}

export interface TableProps {
  configs: Config[]
  isInitiating: boolean
  isStopping: boolean
  initiatePortForwarding: (configs: Config[]) => Promise<void>
  stopAllPortForwarding: () => Promise<void>
  confirmDeleteConfig: () => void
  handleDeleteConfig: (id: number) => void
  handleEditConfig: (id: number) => void
  isAlertOpen: boolean
  setIsAlertOpen: (isOpen: boolean) => void
  selectedConfigs: Config[]
  setSelectedConfigs: React.Dispatch<React.SetStateAction<Config[]>>
  setIsInitiating: React.Dispatch<React.SetStateAction<boolean>>
}

export interface PortForwardRowProps {
  config: Config
  confirmDeleteConfig: () => void
  handleDeleteConfig: (id: number) => void
  handleEditConfig: (id: number) => void
  isAlertOpen: boolean
  setIsAlertOpen: (isOpen: boolean) => void
  showContext?: boolean
  onSelectionChange: (isSelected: boolean) => void
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
  isGitSyncModalOpen: boolean
  selectedConfigs: Config[]
  setPollingInterval: React.Dispatch<React.SetStateAction<number>>
  pollingInterval: number
  setSelectedConfigs: React.Dispatch<React.SetStateAction<Config[]>>
  configs: Config[]
}

export interface SyncConfigsButtonProps {
  serviceName: string
  accountName: string
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

export interface Option {
  name?: string | string
  value: string | number
  label: string
}

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
  [key: string]: Config[]
}

export interface HeaderProps {
  search: string
  setSearch: React.Dispatch<React.SetStateAction<string>>
}

export interface HeaderMenuProps {
  isSelectAllChecked: boolean
  setIsSelectAllChecked: React.Dispatch<React.SetStateAction<boolean>>
  configs: Config[]
  selectedConfigs: Config[]
  initiatePortForwarding: (configs: Config[]) => void
  startSelectedPortForwarding: () => void
  stopAllPortForwarding: () => void
  isInitiating: boolean
  isStopping: boolean
  toggleExpandAll: () => void
  expandedIndices: number[]
  configsByContext: ConfigsByContext
  setSelectedConfigs: React.Dispatch<React.SetStateAction<Config[]>>
}

export interface BulkDeleteButtonProps {
  selectedConfigs: Config[]
  setSelectedConfigs: React.Dispatch<React.SetStateAction<Config[]>>
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
  setIsAlertOpen: (isOpen: boolean) => void
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
