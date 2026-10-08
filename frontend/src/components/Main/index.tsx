import { lazy, Suspense, useCallback, useState } from 'react'

import { Box, VStack } from '@chakra-ui/react'
import { useQuery, useQueryClient } from '@tanstack/react-query'
import { invoke } from '@tauri-apps/api/core'

import { Footer } from '@/components/Footer'
import { useHelperMutations } from '@/components/HelperModal/useHelper'
import { PortForwardTable } from '@/components/PortForwardTable'
import { telemetryQuery } from '@/lib/telemetry'
import type { Config, StoredConfig } from '@/types'

import { useConfigTransfer } from './useConfigTransfer'
import { usePortForwarding } from './usePortForwarding'

const AddConfigModal = lazy(() =>
  import('@/components/AddConfigModal').then(m => ({
    default: m.AddConfigModal,
  })),
)
const AutoImportModal = lazy(() =>
  import('@/components/AutoImportModal').then(m => ({
    default: m.AutoImportModal,
  })),
)
const GitSyncModal = lazy(() =>
  import('@/components/GitSyncModal').then(m => ({ default: m.GitSyncModal })),
)
const HelperModal = lazy(() =>
  import('@/components/HelperModal').then(m => ({ default: m.HelperModal })),
)
const ServerResourcesModal = lazy(() =>
  import('@/components/ServerResourcesModal').then(m => ({
    default: m.ServerResourcesModal,
  })),
)
const SettingsModal = lazy(() =>
  import('@/components/SettingsModal').then(m => ({
    default: m.SettingsModal,
  })),
)
const ShortcutModal = lazy(() =>
  import('@/components/ShortcutModal').then(m => ({
    default: m.ShortcutModal,
  })),
)
const TelemetryConsentModal = lazy(() =>
  import('@/components/TelemetryConsentModal').then(m => ({
    default: m.TelemetryConsentModal,
  })),
)
const PrivilegeDialog = lazy(() =>
  import('@/components/PrivilegeDialog').then(m => ({
    default: m.PrivilegeDialog,
  })),
)

type ActiveModal =
  | { type: 'config'; initialConfig: StoredConfig | null; isEdit: boolean }
  | {
      type:
        | 'autoImport'
        | 'gitSync'
        | 'helper'
        | 'serverResources'
        | 'settings'
        | 'shortcuts'
    }
  | null

export function Main() {
  const [selectedConfigs, setSelectedConfigs] = useState<Config[]>([])
  const [activeModal, setActiveModal] = useState<ActiveModal>(null)
  const queryClient = useQueryClient()
  const telemetry = useQuery(telemetryQuery)
  const forwarding = usePortForwarding()
  const { prompt, resolvePrompt, suppressPrompt } = forwarding.privileges
  const { installMutation } = useHelperMutations()
  const { exportConfigs, importConfigs } = useConfigTransfer()

  const closeModal = () => setActiveModal(null)

  const openConfig = useCallback(
    async (id: number, isEdit: boolean) => {
      const config = await queryClient
        .fetchQuery({
          queryKey: ['config', id],
          queryFn: () => invoke<StoredConfig>('get_config_cmd', { id }),
          staleTime: 0,
          meta: { errorToast: { title: 'Failed to load configuration' } },
        })
        .catch(() => null)

      if (!config) {
        return
      }
      setActiveModal({
        type: 'config',
        isEdit,
        initialConfig: isEdit
          ? config
          : { ...config, id: 0, alias: `${config.alias ?? ''}-copy` },
      })
    },
    [queryClient],
  )

  const editConfig = useCallback(
    (id: number) => openConfig(id, true),
    [openConfig],
  )

  const duplicateConfig = useCallback(
    (id: number) => openConfig(id, false),
    [openConfig],
  )

  const forwardingDelete = forwarding.deleteConfigs
  const deleteConfigs = useCallback(
    async (ids: number[]) => {
      const deleted = await forwardingDelete(ids)

      if (deleted) {
        setSelectedConfigs(prev =>
          prev.filter(config => !ids.includes(config.id)),
        )
      }

      return deleted
    },
    [forwardingDelete],
  )

  return (
    <Box
      position='fixed'
      width='100%'
      height='100%'
      maxHeight='100%'
      maxW='100%'
      overflow='hidden'
      bg='bg.canvas'
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
          bg='bg.canvas'
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
              startSelectedPortForwarding={
                forwarding.startSelectedPortForwarding
              }
              isInitiating={forwarding.isInitiating}
              isStopping={forwarding.isStopping}
              pendingConfigActions={forwarding.pendingConfigActions}
              toggleConfigForward={forwarding.toggleConfigForward}
              handleEditConfig={editConfig}
              handleDuplicateConfig={duplicateConfig}
              stopSelectedPortForwarding={forwarding.stopSelectedPortForwarding}
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
              onOpenHelper={() => setActiveModal({ type: 'helper' })}
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
          {activeModal?.type === 'helper' && (
            <HelperModal onClose={closeModal} />
          )}
          {activeModal?.type === 'settings' && (
            <SettingsModal onClose={closeModal} />
          )}
          {prompt && (
            <PrivilegeDialog
              needs={prompt.needs}
              refusal={prompt.error}
              isInstalling={installMutation.isPending}
              installError={installMutation.error}
              onInstall={() =>
                installMutation.mutate(undefined, {
                  onSuccess: () => resolvePrompt('install'),
                })
              }
              onContinue={() => resolvePrompt('continue')}
              onCancel={() => resolvePrompt('cancel')}
              onSuppress={suppressPrompt}
            />
          )}
          {activeModal === null && !prompt && telemetry.data === null && (
            <TelemetryConsentModal />
          )}
        </Suspense>
      </VStack>
    </Box>
  )
}
