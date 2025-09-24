import React, { useEffect, useState } from 'react'

import {
  Box,
  Dialog,
  Flex,
  Stack,
  Text,
  Wrap,
  WrapItem,
} from '@chakra-ui/react'
import { invoke } from '@tauri-apps/api/core'

import ShortcutCapture from '@/components/ShortcutCapture'
import { Button } from '@/components/ui/button'
import { Checkbox } from '@/components/ui/checkbox'
import { DialogCloseTrigger } from '@/components/ui/dialog'
import { toaster } from '@/components/ui/toaster'
import { type Shortcut, useGlobalShortcuts } from '@/hooks/useGlobalShortcuts'
import { type Config } from '@/types'

interface ShortcutFormModalProps {
  isOpen: boolean
  onClose: () => void
  editingShortcut?: Shortcut | null
  configs: Config[]
  onSaved: () => void
}

interface ShortcutAction {
  id: string
  name: string
  actionType: string
  requiresConfig: boolean
}

const SHORTCUT_ACTIONS: ShortcutAction[] = [
  {
    id: 'toggle_window',
    name: 'Toggle Window',
    actionType: 'toggle_window',
    requiresConfig: false,
  },
  {
    id: 'start_all',
    name: 'Start All Port Forward',
    actionType: 'start_all_port_forward',
    requiresConfig: false,
  },
  {
    id: 'stop_all',
    name: 'Stop All Port Forward',
    actionType: 'stop_all_port_forward',
    requiresConfig: false,
  },
  {
    id: 'start_port_forward',
    name: 'Start Port Forward',
    actionType: 'start_port_forward',
    requiresConfig: true,
  },
  {
    id: 'stop_port_forward',
    name: 'Stop Port Forward',
    actionType: 'stop_port_forward',
    requiresConfig: true,
  },
  {
    id: 'toggle_port_forward',
    name: 'Toggle Port Forward',
    actionType: 'toggle_port_forward',
    requiresConfig: true,
  },
]

const ShortcutFormModal: React.FC<ShortcutFormModalProps> = ({
  isOpen,
  onClose,
  editingShortcut,
  configs,
  onSaved,
}) => {
  const [formData, setFormData] = useState({
    shortcutKey: '',
    actionType: '',
    configIds: [] as number[],
  })
  const [isLoading, setIsLoading] = useState(false)

  const { validateShortcut, normalizeShortcut } = useGlobalShortcuts()

  useEffect(() => {
    if (isOpen) {
      if (editingShortcut) {
        setFormData({
          shortcutKey: editingShortcut.shortcut_key,
          actionType: editingShortcut.action_type,
          configIds: editingShortcut.action_data
            ? (() => {
                try {
                  const data = JSON.parse(editingShortcut.action_data)

                  return data.config_ids || []
                } catch {
                  return []
                }
              })()
            : [],
        })
      } else {
        setFormData({
          shortcutKey: '',
          actionType: '',
          configIds: [],
        })
      }
    }
  }, [isOpen, editingShortcut])

  const selectedAction = SHORTCUT_ACTIONS.find(
    a => a.actionType === formData.actionType,
  )

  const handleSave = async () => {
    if (!formData.shortcutKey || !formData.actionType) {
      toaster.error({
        title: 'Invalid Input',
        description: 'Please set a shortcut key and select an action',
        duration: 3000,
      })

      return
    }

    const action = SHORTCUT_ACTIONS.find(
      a => a.actionType === formData.actionType,
    )

    if (!action) {
      return
    }

    if (action.requiresConfig && formData.configIds.length === 0) {
      toaster.error({
        title: 'Invalid Input',
        description: 'Please select at least one configuration for this action',
        duration: 3000,
      })

      return
    }

    try {
      setIsLoading(true)

      const normalizedShortcut = await normalizeShortcut(formData.shortcutKey)

      if (!normalizedShortcut) {
        toaster.error({
          title: 'Invalid Shortcut',
          description: 'Please enter a valid shortcut format',
          duration: 3000,
        })

        return
      }

      const isValid = await validateShortcut(normalizedShortcut)

      if (!isValid) {
        toaster.error({
          title: 'Invalid Shortcut',
          description: 'The shortcut format is not valid',
          duration: 3000,
        })

        return
      }

      const actionData = action.requiresConfig
        ? JSON.stringify({ config_ids: formData.configIds })
        : undefined

      // Generate unique name by combining action name with shortcut key
      const uniqueName = `${action.name} (${normalizedShortcut})`

      if (editingShortcut) {
        await invoke('update_shortcut', {
          id: editingShortcut.id,
          request: {
            name: uniqueName,
            shortcut_key: normalizedShortcut,
            action_type: action.actionType,
            action_data: actionData,
            enabled: true,
          },
        })
      } else {
        const id = await invoke<number>('create_shortcut', {
          request: {
            name: uniqueName,
            shortcut_key: normalizedShortcut,
            action_type: action.actionType,
            action_data: actionData,
            enabled: true,
          },
        })

        if (!id) {
          toaster.error({
            title: 'Creation Failed',
            description:
              'Failed to create shortcut. It may conflict with another shortcut.',
            duration: 3000,
          })

          return
        }
      }

      toaster.success({
        title: 'Success',
        description: editingShortcut
          ? 'Shortcut updated successfully'
          : 'Shortcut created successfully',
        duration: 3000,
      })

      onSaved()
    } catch (error) {
      console.error('Error saving shortcut:', error)
      toaster.error({
        title: 'Error',
        description: 'Failed to save shortcut',
        duration: 3000,
      })
    } finally {
      setIsLoading(false)
    }
  }

  const handleConfigToggle = (configId: number, checked: boolean) => {
    setFormData(prev => ({
      ...prev,
      configIds: checked
        ? [...prev.configIds, configId]
        : prev.configIds.filter(id => id !== configId),
    }))
  }

  const handleActionSelect = (actionType: string) => {
    const action = SHORTCUT_ACTIONS.find(a => a.actionType === actionType)

    setFormData(prev => ({
      ...prev,
      actionType,
      configIds: action?.requiresConfig ? prev.configIds : [],
    }))
  }

  return (
    <Dialog.Root
      open={isOpen}
      onOpenChange={({ open }) => !open && onClose()}
      modal={true}
      closeOnEscape={true}
    >
      <Dialog.Backdrop
        bg='transparent'
        backdropFilter='blur(4px)'
        height='100vh'
      />
      <Dialog.Positioner overflow='hidden'>
        <Dialog.Content
          onClick={e => e.stopPropagation()}
          maxWidth='600px'
          width='90vw'
          height='96vh'
          bg='#111111'
          borderRadius='lg'
          border='1px solid rgba(255, 255, 255, 0.08)'
          overflow='hidden'
          position='absolute'
          my={2}
          display='flex'
          flexDirection='column'
        >
          <DialogCloseTrigger style={{ marginTop: '-4px' }} />

          <Dialog.Header
            p={3}
            bg='#161616'
            borderBottom='1px solid rgba(255, 255, 255, 0.05)'
          >
            <Text fontSize='sm' fontWeight='medium' color='gray.100'>
              {editingShortcut ? 'Edit Shortcut' : 'Add New Shortcut'}
            </Text>
          </Dialog.Header>

          <Dialog.Body p={3} flex={1} overflowY='auto' overflowX='hidden'>
            <Stack gap={2.5}>
              <Box
                bg='#161616'
                p={2}
                borderRadius='md'
                border='1px solid rgba(255, 255, 255, 0.08)'
              >
                <Text fontSize='xs' color='gray.400' mb={1}>
                  Action Type
                </Text>
                <Wrap gap={1.5}>
                  {SHORTCUT_ACTIONS.map(action => (
                    <WrapItem key={action.id}>
                      <Button
                        size='2xs'
                        variant={
                          formData.actionType === action.actionType
                            ? 'solid'
                            : 'outline'
                        }
                        onClick={() => handleActionSelect(action.actionType)}
                        bg={
                          formData.actionType === action.actionType
                            ? 'blue.500'
                            : 'transparent'
                        }
                        color={
                          formData.actionType === action.actionType
                            ? 'white'
                            : 'whiteAlpha.700'
                        }
                        borderColor='rgba(255, 255, 255, 0.15)'
                        _hover={{
                          borderColor: 'rgba(255, 255, 255, 0.3)',
                          bg:
                            formData.actionType === action.actionType
                              ? 'blue.600'
                              : 'whiteAlpha.100',
                        }}
                        height='20px'
                        fontSize='xs'
                        px={2}
                      >
                        {action.name}
                      </Button>
                    </WrapItem>
                  ))}
                </Wrap>
              </Box>

              <Box
                bg='#161616'
                p={2}
                borderRadius='md'
                border='1px solid rgba(255, 255, 255, 0.08)'
              >
                <Text fontSize='xs' color='gray.400' mb={1}>
                  Keyboard Shortcut
                </Text>
                <ShortcutCapture
                  value={formData.shortcutKey}
                  onChange={key =>
                    setFormData(prev => ({ ...prev, shortcutKey: key }))
                  }
                  disabled={isLoading}
                />
              </Box>

              {selectedAction?.requiresConfig && (
                <Box
                  bg='#161616'
                  p={2}
                  borderRadius='md'
                  border='1px solid rgba(255, 255, 255, 0.08)'
                >
                  <Text fontSize='xs' color='gray.400' mb={1}>
                    Select Configurations
                  </Text>
                  <Box
                    maxHeight='140px'
                    overflowY='auto'
                    overflowX='hidden'
                    bg='#111111'
                    border='1px solid rgba(255, 255, 255, 0.08)'
                    borderRadius='md'
                    p={2}
                    css={{
                      '&::-webkit-scrollbar': { width: '4px' },
                      '&::-webkit-scrollbar-track': {
                        background: 'transparent',
                      },
                      '&::-webkit-scrollbar-thumb': {
                        background: 'rgba(255, 255, 255, 0.2)',
                        borderRadius: '2px',
                      },
                      '&::-webkit-scrollbar-thumb:hover': {
                        background: 'rgba(255, 255, 255, 0.3)',
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
                            bg='rgba(255, 255, 255, 0.03)'
                            border='1px solid rgba(255, 255, 255, 0.05)'
                            borderRadius='sm'
                            p={2}
                            _hover={{ bg: 'rgba(255, 255, 255, 0.05)' }}
                          >
                            <Flex align='center' gap={2}>
                              <Checkbox
                                checked={formData.configIds.includes(config.id)}
                                onCheckedChange={e =>
                                  handleConfigToggle(
                                    config.id,
                                    Boolean(e.checked),
                                  )
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
                  {formData.configIds.length > 0 && (
                    <Text
                      fontSize='xs'
                      color='gray.400'
                      mt={1}
                      lineHeight='1.3'
                    >
                      {formData.configIds.length} configuration
                      {formData.configIds.length !== 1 ? 's' : ''} selected
                    </Text>
                  )}
                </Box>
              )}
            </Stack>
          </Dialog.Body>

          <Dialog.Footer
            px={3}
            py={2}
            bg='#161616'
            borderTop='1px solid rgba(255, 255, 255, 0.05)'
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
                loading={isLoading}
                loadingText={editingShortcut ? 'Updating...' : 'Creating...'}
                bg='blue.500'
                color='white'
                _hover={{ bg: 'blue.600' }}
                _active={{ bg: 'blue.700' }}
                height='28px'
                fontSize='xs'
              >
                {editingShortcut ? 'Save Changes' : 'Add Shortcut'}
              </Button>
            </Flex>
          </Dialog.Footer>
        </Dialog.Content>
      </Dialog.Positioner>
    </Dialog.Root>
  )
}

export default ShortcutFormModal
