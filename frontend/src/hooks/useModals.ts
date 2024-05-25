import { useState } from 'react'

import { Config } from '../types'

const initialConfig: Config = {
  id: 0,
  service: '',
  context: '',
  local_port: 0,
  local_address: '127.0.0.1',
  domain_enabled: false,
  remote_port: 0,
  namespace: '',
  workload_type: '',
  protocol: '',
  remote_address: '',
  alias: '',
  kubeconfig: 'default',
}

const useModals = () => {
  const [isModalOpen, setIsModalOpen] = useState(false)
  const [isGitSyncModalOpen, setIsGitSyncModalOpen] = useState(false)
  const [newConfig, setNewConfig] = useState<Config>(initialConfig)
  const [isEdit, setIsEdit] = useState(false)

  const openModal = () => {
    setNewConfig(initialConfig)
    setIsEdit(false)
    setIsModalOpen(true)
  }

  const closeModal = () => {
    setIsModalOpen(false)
    setIsEdit(false)
  }

  const openGitSyncModal = () => {
    setIsGitSyncModalOpen(true)
  }

  const closeGitSyncModal = () => {
    setIsGitSyncModalOpen(false)
  }

  return {
    setIsModalOpen,
    isModalOpen,
    openModal,
    closeModal,
    isGitSyncModalOpen,
    openGitSyncModal,
    closeGitSyncModal,
    newConfig,
    setNewConfig,
    isEdit,
    setIsEdit,
  }
}

export default useModals
