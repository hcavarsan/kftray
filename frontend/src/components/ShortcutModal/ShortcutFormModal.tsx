import { useState } from 'react'

import { Box, Flex, Stack, Text, Wrap, WrapItem } from '@chakra-ui/react'
import { useMutation, useQueryClient } from '@tanstack/react-query'

import { ShortcutCapture } from '@/components/ShortcutCapture'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import {
  AppDialog,
  AppDialogBody,
  AppDialogFooter,
  DialogCancelButton,
} from '@/components/ui/dialog'
import { showErrorToast } from '@/components/ui/error-toast'
import { toaster } from '@/components/ui/toaster'
import { type Shortcut, shortcutsQuery } from '@/hooks/useGlobalShortcuts'
import { invoke } from '@/lib/tauri'
import type { Config } from '@/types'

import {
  findShortcutAction,
  SHORTCUT_ACTIONS,
  type ShortcutActionType,
  shortcutConfigIds,
} from './actions'

interface ShortcutFormModalProps {
  shortcut: Shortcut | null
  configs: Config[]
  onClose: () => void
}

interface ShortcutDraft {
  shortcutKey: string
  actionType: ShortcutActionType | null
  configIds: number[]
}

class ShortcutFormError extends Error {
  title: string

  constructor(title: string, message: string) {
    super(message)
    this.title = title
  }
}

export function ShortcutFormModal({
  shortcut,
  configs,
  onClose,
}: ShortcutFormModalProps) {
  const queryClient = useQueryClient()
  const [draft, setDraft] = useState<ShortcutDraft>(() => ({
    shortcutKey: shortcut?.shortcut_key ?? '',
    actionType:
      findShortcutAction(shortcut?.action_type ?? '')?.actionType ?? null,
    configIds: shortcutConfigIds(shortcut?.action_data),
  }))
  const selectedAction = draft.actionType
    ? findShortcutAction(draft.actionType)
    : undefined

  const save = useMutation({
    mutationFn: async () => {
      if (!selectedAction) {
        throw new ShortcutFormError(
          'Invalid Input',
          'Please set a shortcut key and select an action',
        )
      }
      const normalized = await invoke<string>('normalize_shortcut_key', {
        shortcutStr: draft.shortcutKey,
      }).catch(() => {
        throw new ShortcutFormError(
          'Invalid Shortcut',
          'Please enter a valid shortcut format',
        )
      })
      const isValid = await invoke<boolean>('validate_shortcut_key', {
        shortcutKey: normalized,
      }).catch(() => false)

      if (!isValid) {
        throw new ShortcutFormError(
          'Invalid Shortcut',
          'The shortcut format is not valid',
        )
      }

      const request = {
        name: `${selectedAction.name} (${normalized})`,
        shortcut_key: normalized,
        action_type: selectedAction.actionType,
        action_data: selectedAction.requiresConfig
          ? JSON.stringify({ config_ids: draft.configIds })
          : undefined,
        enabled: true,
      }

      if (shortcut) {
        await invoke('update_shortcut', { id: shortcut.id, request })
      } else {
        const id = await invoke<number>('create_shortcut', { request })

        if (!id) {
          throw new ShortcutFormError(
            'Creation Failed',
            'Failed to create shortcut. It may conflict with another shortcut.',
          )
        }
      }
    },
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: shortcutsQuery.queryKey })
      toaster.success({
        title: 'Success',
        description: shortcut
          ? 'Shortcut updated successfully'
          : 'Shortcut created successfully',
        duration: 3000,
      })
      onClose()
    },
    onError: error => {
      if (error instanceof ShortcutFormError) {
        toaster.error({
          title: error.title,
          description: error.message,
          duration: 3000,
        })

        return
      }
      showErrorToast(error, {
        errorToast: {
          title: 'Error',
          description: 'Failed to save shortcut',
          duration: 3000,
        },
      })
    },
  })

  const handleSave = () => {
    if (!draft.shortcutKey || !selectedAction) {
      toaster.error({
        title: 'Invalid Input',
        description: 'Please set a shortcut key and select an action',
        duration: 3000,
      })

      return
    }
    if (selectedAction.requiresConfig && draft.configIds.length === 0) {
      toaster.error({
        title: 'Invalid Input',
        description: 'Please select at least one configuration for this action',
        duration: 3000,
      })

      return
    }
    save.mutate()
  }

  const toggleConfig = (configId: number, checked: boolean) =>
    setDraft(prev => ({
      ...prev,
      configIds: checked
        ? [...prev.configIds, configId]
        : prev.configIds.filter(id => id !== configId),
    }))

  const selectAction = (actionType: ShortcutActionType) =>
    setDraft(prev => ({
      ...prev,
      actionType,
      configIds: findShortcutAction(actionType)?.requiresConfig
        ? prev.configIds
        : [],
    }))

  return (
    <AppDialog
      title={shortcut ? 'Edit Shortcut' : 'Add New Shortcut'}
      onClose={onClose}
      maxWidth='600px'
      height='96vh'
    >
      <AppDialogBody overflowX='hidden'>
        <Stack gap={2.5}>
          <Box
            bg='bg.surface'
            p={2}
            borderRadius='md'
            border='1px solid'
            borderColor='border'
          >
            <Text fontSize='xs' color='fg.muted' mb={1}>
              Action Type
            </Text>
            <Wrap gap={1.5}>
              {SHORTCUT_ACTIONS.map(action => {
                const selected = draft.actionType === action.actionType

                return (
                  <WrapItem key={action.actionType}>
                    <Button
                      size='2xs'
                      variant={selected ? 'solid' : 'outline'}
                      onClick={() => selectAction(action.actionType)}
                      bg={selected ? 'accent.solid' : 'transparent'}
                      color={selected ? 'fg' : 'fg.muted'}
                      borderColor='border.emphasized'
                      _hover={{
                        borderColor: 'border.strong',
                        bg: selected ? 'accent.solidHover' : 'bg.hover',
                      }}
                      height='20px'
                      fontSize='xs'
                      px={2}
                    >
                      {action.name}
                    </Button>
                  </WrapItem>
                )
              })}
            </Wrap>
          </Box>

          <Box
            bg='bg.surface'
            p={2}
            borderRadius='md'
            border='1px solid'
            borderColor='border'
          >
            <Text fontSize='xs' color='fg.muted' mb={1}>
              Keyboard Shortcut
            </Text>
            <ShortcutCapture
              value={draft.shortcutKey}
              onChange={shortcutKey =>
                setDraft(prev => ({ ...prev, shortcutKey }))
              }
              disabled={save.isPending}
            />
          </Box>

          {selectedAction?.requiresConfig && (
            <Box
              bg='bg.surface'
              p={2}
              borderRadius='md'
              border='1px solid'
              borderColor='border'
            >
              <Text fontSize='xs' color='fg.muted' mb={1}>
                Select Configurations
              </Text>
              <Box
                maxHeight='140px'
                overflowY='auto'
                overflowX='hidden'
                bg='bg.canvas'
                border='1px solid'
                borderColor='border'
                borderRadius='md'
                p={2}
              >
                {configs.length === 0 ? (
                  <Text
                    fontSize='xs'
                    color='fg.muted'
                    textAlign='center'
                    lineHeight='1.3'
                  >
                    No configurations available
                  </Text>
                ) : (
                  <Stack gap={1}>
                    {configs.map(config => (
                      <Box
                        key={config.id}
                        bg='bg.faint'
                        border='1px solid'
                        borderColor='border.subtle'
                        borderRadius='sm'
                        p={2}
                        _hover={{ bg: 'bg.hover' }}
                      >
                        <Flex align='center' gap={2}>
                          <Checkbox
                            checked={draft.configIds.includes(config.id)}
                            onCheckedChange={e =>
                              toggleConfig(config.id, e.checked === true)
                            }
                            size='sm'
                          />
                          <Box flex={1}>
                            <Text fontSize='xs' color='fg' fontWeight='medium'>
                              {config.alias}
                            </Text>
                            <Text
                              fontSize='xs'
                              color='fg.muted'
                              lineHeight='1.3'
                            >
                              {config.context} / {config.namespace}
                            </Text>
                          </Box>
                        </Flex>
                      </Box>
                    ))}
                  </Stack>
                )}
              </Box>
              {draft.configIds.length > 0 && (
                <Text fontSize='xs' color='fg.muted' mt={1} lineHeight='1.3'>
                  {draft.configIds.length} configuration
                  {draft.configIds.length !== 1 ? 's' : ''} selected
                </Text>
              )}
            </Box>
          )}
        </Stack>
      </AppDialogBody>

      <AppDialogFooter>
        <DialogCancelButton onClick={onClose} />
        <Button
          size='xs'
          onClick={handleSave}
          loading={save.isPending}
          loadingText={shortcut ? 'Updating...' : 'Creating...'}
          bg='accent.solid'
          color='fg'
          _hover={{ bg: 'accent.solidHover' }}
          _active={{ bg: 'accent.solidActive' }}
          height='28px'
          fontSize='xs'
        >
          {shortcut ? 'Save Changes' : 'Add Shortcut'}
        </Button>
      </AppDialogFooter>
    </AppDialog>
  )
}
