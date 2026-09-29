import { useState } from 'react'

import {
  Box,
  Dialog,
  Flex,
  Stack,
  Text,
  Wrap,
  WrapItem,
} from '@chakra-ui/react'
import { useMutation, useQueryClient } from '@tanstack/react-query'
import { invoke } from '@tauri-apps/api/core'

import ShortcutCapture from '@/components/ShortcutCapture'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { AppDialog } from '@/components/ui/dialog'
import { toaster } from '@/components/ui/toaster'
import { type Shortcut, shortcutsQuery } from '@/hooks/useGlobalShortcuts'
import { errorMessage } from '@/lib/errors'
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

class ShortcutInputError extends Error {}

export default function ShortcutFormModal({
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
        throw new ShortcutInputError(
          'Please set a shortcut key and select an action',
        )
      }
      const normalized = await invoke<string>('normalize_shortcut_key', {
        shortcutStr: draft.shortcutKey,
      }).catch(() => {
        throw new ShortcutInputError('Please enter a valid shortcut format')
      })
      const isValid = await invoke<boolean>('validate_shortcut_key', {
        shortcutKey: normalized,
      })

      if (!isValid) {
        throw new ShortcutInputError('The shortcut format is not valid')
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
          throw new Error(
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
    onError: error =>
      toaster.error({
        title:
          error instanceof ShortcutInputError
            ? 'Invalid Shortcut'
            : 'Failed to save shortcut',
        description: errorMessage(error),
        duration: 3000,
      }),
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
      <Dialog.Body p={3} flex={1} overflowY='auto' overflowX='hidden'>
        <Stack gap={2.5}>
          <Box
            bg='app.panel'
            p={2}
            borderRadius='md'
            border='1px solid'
            borderColor='app.border'
          >
            <Text fontSize='xs' color='gray.400' mb={1}>
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
                      bg={selected ? 'blue.500' : 'transparent'}
                      color={selected ? 'white' : 'whiteAlpha.700'}
                      borderColor='app.borderStrong'
                      _hover={{
                        borderColor: 'white/30',
                        bg: selected ? 'blue.600' : 'whiteAlpha.100',
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
            bg='app.panel'
            p={2}
            borderRadius='md'
            border='1px solid'
            borderColor='app.border'
          >
            <Text fontSize='xs' color='gray.400' mb={1}>
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
              bg='app.panel'
              p={2}
              borderRadius='md'
              border='1px solid'
              borderColor='app.border'
            >
              <Text fontSize='xs' color='gray.400' mb={1}>
                Select Configurations
              </Text>
              <Box
                maxHeight='140px'
                overflowY='auto'
                overflowX='hidden'
                bg='app.bg'
                border='1px solid'
                borderColor='app.border'
                borderRadius='md'
                p={2}
                css={{
                  '&::-webkit-scrollbar': { width: '4px' },
                  '&::-webkit-scrollbar-track': {
                    background: 'transparent',
                  },
                  '&::-webkit-scrollbar-thumb': {
                    background: 'var(--chakra-colors-app-divider)',
                    borderRadius: '2px',
                  },
                }}
              >
                {configs.length === 0 ? (
                  <Text
                    fontSize='xs'
                    color='gray.400'
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
                        bg='app.faint'
                        border='1px solid'
                        borderColor='app.hover'
                        borderRadius='sm'
                        p={2}
                        _hover={{ bg: 'app.hover' }}
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
                            <Text
                              fontSize='xs'
                              color='gray.100'
                              fontWeight='medium'
                            >
                              {config.alias}
                            </Text>
                            <Text
                              fontSize='xs'
                              color='gray.400'
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
                <Text fontSize='xs' color='gray.400' mt={1} lineHeight='1.3'>
                  {draft.configIds.length} configuration
                  {draft.configIds.length !== 1 ? 's' : ''} selected
                </Text>
              )}
            </Box>
          )}
        </Stack>
      </Dialog.Body>

      <Dialog.Footer
        px={3}
        py={2}
        bg='app.panel'
        borderTop='1px solid'
        borderColor='app.hover'
        flexShrink={0}
      >
        <Flex justify='flex-end' gap={2} width='100%'>
          <Button
            variant='ghost'
            size='xs'
            onClick={onClose}
            _hover={{ bg: 'whiteAlpha.50' }}
            color='gray.400'
            height='28px'
            fontSize='xs'
          >
            Cancel
          </Button>
          <Button
            size='xs'
            onClick={handleSave}
            loading={save.isPending}
            loadingText={shortcut ? 'Updating...' : 'Creating...'}
            bg='blue.500'
            color='white'
            _hover={{ bg: 'blue.600' }}
            _active={{ bg: 'blue.700' }}
            height='28px'
            fontSize='xs'
          >
            {shortcut ? 'Save Changes' : 'Add Shortcut'}
          </Button>
        </Flex>
      </Dialog.Footer>
    </AppDialog>
  )
}
