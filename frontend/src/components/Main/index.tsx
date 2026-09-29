import { lazy, Suspense, useState } from 'react'

import { Box, VStack } from '@chakra-ui/react'
import { invoke } from '@tauri-apps/api/core'

import Footer from '@/components/Footer'
import PortForwardTable from '@/components/PortForwardTable'
import { toaster } from '@/components/ui/toaster'
import { errorMessage } from '@/lib/errors'
import type { Config } from '@/types'

import { useConfigTransfer } from './useConfigTransfer'
import { usePortForwarding } from './usePortForwarding'

const AddConfigModal = lazy(() => import('@/components/AddConfigModal'))
const AutoImportModal = lazy(() => import('@/components/AutoImportModal'))
const GitSyncModal = lazy(() => import('@/components/GitSyncModal'))
const ServerResourcesModal = lazy(
  () => import('@/components/ServerResourcesModal'),
)
const SettingsModal = lazy(() => import('@/components/SettingsModal'))
const ShortcutModal = lazy(() => import('@/components/ShortcutModal'))

type ActiveModal =
  | { type: 'config'; initialConfig: Config | null; isEdit: boolean }
  | {
      type:
        | 'autoImport'
        | 'gitSync'
        | 'serverResources'
        | 'settings'
        | 'shortcuts'
    }
  | null

function Main() {
  const [selectedConfigs, setSelectedConfigs] = useState<Config[]>([])
  const [activeModal, setActiveModal] = useState<ActiveModal>(null)
  const forwarding = usePortForwarding()
  const { exportConfigs, importConfigs } = useConfigTransfer()

  const closeModal = () => setActiveModal(null)

  const openConfig = async (id: number, isEdit: boolean) => {
    try {
      const config = await invoke<Config>('get_config_cmd', { id })

      setActiveModal({
        type: 'config',
        isEdit,
        initialConfig: isEdit
          ? config
          : {
              ...config,
              id: 0,
              alias: `${config.alias}-copy`,
              is_running: false,
            },
      })
    } catch (error) {
      toaster.error({
        title: 'Failed to load configuration',
        description: errorMessage(error),
      })
    }
  }

  const deleteConfigs = async (ids: number[]) => {
    const deleted = await forwarding.deleteConfigs(ids)

    if (deleted) {
      setSelectedConfigs(prev =>
        prev.filter(config => !ids.includes(config.id)),
      )
    }

    return deleted
  }

  return (
    <Box
      position='fixed'
      width='100%'
      height='100%'
      maxHeight='100%'
      maxW='100%'
      overflow='hidden'
      bg='app.bg'
      borderRadius='lg'
    >
      <VStack
        height='100%'
        width='100%'
        gap={0}
        position='relative'
        overflow='hidden'
      >
        <Box
          flex={1}
          width='100%'
          height='100%'
          position='relative'
          overflow='hidden'
          bg='app.bg'
        >
          <Box
            position='absolute'
            top={0}
            left={0}
            right={0}
            bottom='60px'
            overflow='auto'
            padding='5px'
          >
            <PortForwardTable
              configs={forwarding.configs}
              initiatePortForwarding={forwarding.initiatePortForwarding}
              startSelectedPortForwarding={() =>
                forwarding.startSelectedPortForwarding(selectedConfigs)
              }
              isInitiating={forwarding.isInitiating}
              isStopping={forwarding.isStopping}
              pendingConfigActions={forwarding.pendingConfigActions}
              toggleConfigForward={forwarding.toggleConfigForward}
              handleEditConfig={id => openConfig(id, true)}
              handleDuplicateConfig={id => openConfig(id, false)}
              stopSelectedPortForwarding={() =>
                forwarding.stopSelectedPortForwarding(selectedConfigs)
              }
              stopAllPortForwarding={forwarding.stopAllPortForwarding}
              abortStartOperation={forwarding.abortStartOperation}
              abortStopOperation={forwarding.abortStopOperation}
              deleteConfigs={deleteConfigs}
              selectedConfigs={selectedConfigs}
              setSelectedConfigs={setSelectedConfigs}
            />
          </Box>

          <Box
            position='absolute'
            left={0}
            right={0}
            bottom={0}
            overflow='hidden'
            padding='5px'
            zIndex={1}
          >
            <Footer
              selectedConfigs={selectedConfigs}
              deleteConfigs={deleteConfigs}
              onAddConfig={() =>
                setActiveModal({
                  type: 'config',
                  initialConfig: null,
                  isEdit: false,
                })
              }
              onImportConfigs={importConfigs}
              onExportConfigs={exportConfigs}
              onOpenGitSync={() => setActiveModal({ type: 'gitSync' })}
              onOpenAutoImport={() => setActiveModal({ type: 'autoImport' })}
              onOpenShortcuts={() => setActiveModal({ type: 'shortcuts' })}
              onOpenSettings={() => setActiveModal({ type: 'settings' })}
              onOpenServerResources={() =>
                setActiveModal({ type: 'serverResources' })
              }
            />
          </Box>
        </Box>

        <Suspense fallback={null}>
          {activeModal?.type === 'config' && (
            <AddConfigModal
              initialConfig={activeModal.initialConfig}
              isEdit={activeModal.isEdit}
              onClose={closeModal}
              onSave={config =>
                forwarding.saveConfig(config, activeModal.isEdit)
              }
            />
          )}
          {activeModal?.type === 'gitSync' && (
            <GitSyncModal onClose={closeModal} />
          )}
          {activeModal?.type === 'autoImport' && (
            <AutoImportModal onClose={closeModal} />
          )}
          {activeModal?.type === 'shortcuts' && (
            <ShortcutModal onClose={closeModal} />
          )}
          {activeModal?.type === 'serverResources' && (
            <ServerResourcesModal onClose={closeModal} />
          )}
          {activeModal?.type === 'settings' && (
            <SettingsModal onClose={closeModal} />
          )}
        </Suspense>
      </VStack>
    </Box>
  )
}

export default Main
