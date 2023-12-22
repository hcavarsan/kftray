import { RefObject } from 'react'

export interface Status {
  id: number
  service: string
  context: string
  local_port: number
  isRunning: boolean
  namespace: string
  remote_port: number
  cancelRef?: RefObject<HTMLButtonElement>
}

export interface Config {
  id: number
  service: string
  namespace: string
  local_port: number
  remote_port: number
  context: string
}

export interface Response {
  id: number
  service: string
  context: string
  local_port: number
  status: number
  namespace: string
  remote_port: number
  stdout: string
  stderr: string
}

export interface ConfigProps {
  isModalOpen: boolean
  closeModal: () => void
  newConfig: Config
  handleInputChange: (event: React.ChangeEvent<HTMLInputElement>) => void
  handleSaveConfig: (e: React.FormEvent) => Promise<void>
  handleEditSubmit: (e: React.FormEvent) => Promise<void>
  cancelRef: RefObject<HTMLElement>
  isEdit: boolean
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
}

export interface PortForwardRowProps {
  config: Status
  confirmDeleteConfig: () => void
  handleDeleteConfig: (id: number) => void
  handleEditConfig: (id: number) => void
  isAlertOpen: boolean
  setIsAlertOpen: (isOpen: boolean) => void
  updateConfigRunningState: (id: number, isRunning: boolean) => void
}

export interface FooterProps {
  openModal: () => void
  handleExportConfigs: () => void
  handleImportConfigs: () => void
}
